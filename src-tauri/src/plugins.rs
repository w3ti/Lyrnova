use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::RwLock,
};

use serde::{Deserialize, Serialize};
use tauri::Manager;

pub const CODEX_PLUGIN_ID: &str = "io.github.w3ti.lyrnova.ai.codex";
const PLUGIN_STATE_VERSION: u32 = 2;
const MAX_PLUGIN_STATE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Language,
    AiProvider,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub kind: PluginKind,
    pub installed: bool,
    pub enabled: bool,
    pub official: bool,
    pub repository: String,
    pub capabilities: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Clone, Copy)]
struct CatalogPlugin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    version: &'static str,
    kind: PluginKind,
    repository: &'static str,
    capabilities: &'static [&'static str],
    permissions: &'static [&'static str],
}

const CATALOG: &[CatalogPlugin] = &[
    CatalogPlugin {
        id: "io.github.w3ti.lyrnova.language.rust",
        name: "Rust",
        description: "Suporte oficial a projetos Rust e Cargo.",
        version: "0.1.0",
        kind: PluginKind::Language,
        repository: "https://github.com/w3ti/lyrnova",
        capabilities: &[
            "Syntax",
            "Autocomplete",
            "Snippets",
            "Templates",
            "LSP planejado",
            "Debug planejado",
        ],
        permissions: &[
            "Ler arquivos do workspace",
            "Iniciar ferramentas autorizadas",
        ],
    },
    CatalogPlugin {
        id: "io.github.w3ti.lyrnova.language.web",
        name: "Web Essentials",
        description: "HTML, CSS, JavaScript, TypeScript e templates web.",
        version: "0.1.0",
        kind: PluginKind::Language,
        repository: "https://github.com/w3ti/lyrnova",
        capabilities: &["Syntax", "Autocomplete", "Validação", "Templates"],
        permissions: &["Ler arquivos do workspace"],
    },
    CatalogPlugin {
        id: CODEX_PLUGIN_ID,
        name: "Codex",
        description: "Chat de programação com conta OpenAI e approvals do Lyrnova.",
        version: "0.1.0",
        kind: PluginKind::AiProvider,
        repository: "https://github.com/w3ti/lyrnova",
        capabilities: &[
            "Conta OpenAI",
            "Chat remoto",
            "Streaming",
            "Tools",
            "Approvals",
        ],
        permissions: &[
            "Acessar o Codex App Server local",
            "Rede mediada pelo provider",
            "Solicitar approvals",
        ],
    },
];

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PluginPreferences {
    version: u32,
    installed: BTreeSet<String>,
    enabled: BTreeSet<String>,
}

impl Default for PluginPreferences {
    fn default() -> Self {
        let defaults: BTreeSet<_> = CATALOG
            .iter()
            .filter(|plugin| plugin.kind == PluginKind::Language)
            .map(|plugin| plugin.id.to_owned())
            .collect();
        Self {
            version: PLUGIN_STATE_VERSION,
            installed: defaults.clone(),
            enabled: defaults,
        }
    }
}

#[derive(Debug, Default)]
pub struct PluginRegistry {
    preferences: RwLock<PluginPreferences>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PluginError {
    UnknownPlugin,
    NotInstalled,
    InvalidRepository,
    StateUnavailable,
    Io,
}

impl PluginRegistry {
    pub fn load(&self, app: &tauri::AppHandle) {
        let Some(preferences) = read_preferences(app) else {
            return;
        };
        if let Ok(mut current) = self.preferences.write() {
            *current = preferences;
        }
    }

    pub fn list(&self) -> Result<Vec<PluginSummary>, PluginError> {
        let preferences = self
            .preferences
            .read()
            .map_err(|_| PluginError::StateUnavailable)?;
        Ok(CATALOG
            .iter()
            .map(|plugin| PluginSummary {
                id: plugin.id.into(),
                name: plugin.name.into(),
                description: plugin.description.into(),
                version: plugin.version.into(),
                kind: plugin.kind,
                installed: preferences.installed.contains(plugin.id),
                enabled: preferences.enabled.contains(plugin.id),
                official: true,
                repository: plugin.repository.into(),
                capabilities: plugin
                    .capabilities
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                permissions: plugin
                    .permissions
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
            })
            .collect())
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.preferences.read().is_ok_and(|preferences| {
            preferences.installed.contains(id) && preferences.enabled.contains(id)
        })
    }

    pub fn install(
        &self,
        app: &tauri::AppHandle,
        id: &str,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        catalog_plugin(id)?;
        {
            let mut preferences = self
                .preferences
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            preferences.installed.insert(id.to_owned());
            preferences.enabled.insert(id.to_owned());
            write_preferences(app, &preferences)?;
        }
        self.list()
    }

    pub fn uninstall(
        &self,
        app: &tauri::AppHandle,
        id: &str,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        catalog_plugin(id)?;
        {
            let mut preferences = self
                .preferences
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            if !preferences.installed.remove(id) {
                return Err(PluginError::NotInstalled);
            }
            preferences.enabled.remove(id);
            write_preferences(app, &preferences)?;
        }
        self.list()
    }

    pub fn set_enabled(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        enabled: bool,
    ) -> Result<Vec<PluginSummary>, PluginError> {
        catalog_plugin(id)?;
        {
            let mut preferences = self
                .preferences
                .write()
                .map_err(|_| PluginError::StateUnavailable)?;
            if !preferences.installed.contains(id) {
                return Err(PluginError::NotInstalled);
            }
            if enabled {
                preferences.enabled.insert(id.to_owned());
            } else {
                preferences.enabled.remove(id);
            }
            write_preferences(app, &preferences)?;
        }
        self.list()
    }

    pub fn open_repository(&self, id: &str) -> Result<(), PluginError> {
        let repository = catalog_plugin(id)?.repository;
        if !repository.starts_with("https://github.com/w3ti/") {
            return Err(PluginError::InvalidRepository);
        }
        #[cfg(target_os = "linux")]
        let mut command = Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut command = Command::new("open");
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("rundll32.exe");
            command.arg("url.dll,FileProtocolHandler");
            command
        };
        command
            .arg(repository)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|_| PluginError::Io)
    }
}

fn catalog_plugin(id: &str) -> Result<&'static CatalogPlugin, PluginError> {
    CATALOG
        .iter()
        .find(|plugin| plugin.id == id)
        .ok_or(PluginError::UnknownPlugin)
}

fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, PluginError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("plugins.json"))
        .map_err(|_| PluginError::Io)
}

fn read_preferences(app: &tauri::AppHandle) -> Option<PluginPreferences> {
    let path = preferences_path(app).ok()?;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PLUGIN_STATE_BYTES {
        return None;
    }
    let mut preferences: PluginPreferences = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    if preferences.version != PLUGIN_STATE_VERSION {
        return None;
    }
    let known: BTreeSet<_> = CATALOG.iter().map(|plugin| plugin.id.to_owned()).collect();
    preferences.installed.retain(|id| known.contains(id));
    preferences
        .enabled
        .retain(|id| preferences.installed.contains(id));
    Some(preferences)
}

fn write_preferences(
    app: &tauri::AppHandle,
    preferences: &PluginPreferences,
) -> Result<(), PluginError> {
    let path = preferences_path(app)?;
    let parent = path.parent().ok_or(PluginError::Io)?;
    fs::create_dir_all(parent).map_err(|_| PluginError::Io)?;
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(preferences).map_err(|_| PluginError::Io)?;
    fs::write(&temporary, bytes).map_err(|_| PluginError::Io)?;
    fs::rename(&temporary, path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        PluginError::Io
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_unique_namespaced_ids_and_https_github_repositories() {
        let mut ids = BTreeSet::new();
        for plugin in CATALOG {
            assert!(ids.insert(plugin.id));
            assert!(plugin.id.starts_with("io.github.w3ti.lyrnova."));
            assert!(plugin.repository.starts_with("https://github.com/w3ti/"));
        }
    }

    #[test]
    fn defaults_install_languages_without_an_ai_provider() {
        let preferences = PluginPreferences::default();
        assert!(
            preferences
                .installed
                .contains("io.github.w3ti.lyrnova.language.rust")
        );
        assert!(
            preferences
                .installed
                .contains("io.github.w3ti.lyrnova.language.web")
        );
        assert!(!preferences.installed.contains(CODEX_PLUGIN_ID));
        assert!(!preferences.enabled.contains(CODEX_PLUGIN_ID));
    }
}
