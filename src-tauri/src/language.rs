//! Versioned Rust diagnostics and read-only symbol queries. No generic LSP IPC.
mod environment;
mod protocol;
pub use environment::{EnvironmentChoice, EnvironmentReview, EnvironmentSummary};
mod editing;
mod query;
pub use query::{LanguageQuery, QueryResult};
#[cfg(target_os = "linux")]
mod runtime;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use url::Url;

pub const RUST_PLUGIN_ID: &str = "io.github.w3ti.lyrnova.language.rust";
const MAX_DOCUMENT: usize = 512 * 1024;
const MAX_DOCUMENTS: usize = 32;
const MAX_TOTAL: usize = 8 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum LanguageError {
    PermissionDenied,
    NoWorkspace,
    WorkspaceChanged,
    StaleSession,
    InvalidDocument,
    TooLarge,
    ServerUnavailable,
    ToolchainUnavailable,
    ReviewExpired,
    SandboxUnavailable,
    SpawnFailed,
    ProtocolViolation,
    Timeout,
    ServerExited,
    StateUnavailable,
    UnsupportedPlatform,
    UnsupportedFeature,
    StaleDocument,
    Cancelled,
    Busy,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanguageDocument {
    pub path: String,
    pub version: i32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}
#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: u8,
    pub message: String,
    pub code: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DocumentDiagnostics {
    pub path: String,
    pub version: i32,
    pub items: Vec<Diagnostic>,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageSnapshot {
    pub session_id: String,
    pub state: String,
    pub error: Option<LanguageError>,
    pub diagnostics: Vec<DocumentDiagnostics>,
    pub environment: Option<EnvironmentSummary>,
    pub analysis_message: Option<String>,
    pub document_versions: BTreeMap<String, i32>,
}
struct State {
    sources: Vec<environment::SourceRoot>,
    configuration: Value,
    queries: query::Queries,
    highest_version: i32,
    documents: BTreeMap<String, Arc<LanguageDocument>>,
    snapshot: LanguageSnapshot,
}
struct Session {
    root: PathBuf,
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[derive(Default)]
pub struct LanguageService {
    session: Mutex<Option<Session>>,
    environments: Mutex<environment::Environments>,
}
impl LanguageService {
    pub fn invalidate_disk(&self, root: &Path) -> Result<(), LanguageError> {
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        if let Some(current) = session.as_ref().filter(|s| s.root == root) {
            let mut state = current
                .state
                .lock()
                .map_err(|_| LanguageError::StateUnavailable)?;
            if matches!(
                state.snapshot.state.as_str(),
                "running" | "starting" | "outdated"
            ) {
                state.snapshot.state = "outdated".into();
                state.snapshot.diagnostics.clear();
                state.queries.revision += 1;
                state.queries.stop();
            }
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<(), LanguageError> {
        *self
            .environments
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)? = environment::Environments::default();
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?
            .take();
        drop(session);
        Ok(())
    }
    pub fn start(&self, root: &Path, restart: bool) -> Result<LanguageSnapshot, LanguageError> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        if let Some(current) = session.as_ref().filter(|s| !restart && s.root == root) {
            return snapshot(current);
        }
        drop(session.take());
        #[cfg(not(target_os = "linux"))]
        {
            let _ = root;
            Err(LanguageError::UnsupportedPlatform)
        }
        #[cfg(target_os = "linux")]
        {
            let environment = self.selected_environment(root)?;
            let child = runtime::spawn(root, &environment)?;
            let state = Arc::new(Mutex::new(State {
                sources: environment.sources(),
                configuration: environment.configuration(),
                queries: query::Queries::default(),
                highest_version: 0,
                documents: BTreeMap::new(),
                snapshot: LanguageSnapshot {
                    session_id: uuid::Uuid::new_v4().to_string(),
                    state: "starting".into(),
                    error: None,
                    diagnostics: vec![],
                    environment: Some(environment.summary()),
                    analysis_message: None,
                    document_versions: BTreeMap::new(),
                },
            }));
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_state = Arc::clone(&state);
            let worker_cancel = Arc::clone(&cancel);
            let worker =
                std::thread::spawn(move || runtime::run(child, worker_state, worker_cancel));
            let current = Session {
                root: root.into(),
                state,
                cancel,
                worker: Some(worker),
            };
            let result = snapshot(&current);
            *session = Some(current);
            result
        }
    }
    pub fn snapshot(&self, root: &Path, id: &str) -> Result<LanguageSnapshot, LanguageError> {
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        let current = matching(&session, root, id)?;
        snapshot(current)
    }
    pub fn sync(
        &self,
        root: &Path,
        id: &str,
        documents: Vec<LanguageDocument>,
    ) -> Result<(), LanguageError> {
        let documents = validate_documents(root, documents)?;
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        let current = matching(&session, root, id)?;
        let mut state = current
            .state
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        // A global high-water mark also protects files closed and reopened in this session.
        for (path, doc) in &documents {
            match state.documents.get(path) {
                Some(old) if old.version == doc.version && old.text == doc.text => {}
                _ if doc.version <= state.highest_version => {
                    return Err(LanguageError::InvalidDocument);
                }
                _ => {}
            }
        }
        state.highest_version = documents
            .values()
            .map(|d| d.version)
            .max()
            .unwrap_or(0)
            .max(state.highest_version);
        state.snapshot.diagnostics.retain(|d| {
            documents
                .get(&d.path)
                .is_some_and(|doc| doc.version == d.version)
        });
        if state.documents != documents {
            state.queries.revision += 1;
        }
        state.documents = documents;
        Ok(())
    }
}
fn matching<'a>(
    session: &'a Option<Session>,
    root: &Path,
    id: &str,
) -> Result<&'a Session, LanguageError> {
    let current = session.as_ref().ok_or(LanguageError::StaleSession)?;
    if current.root != root {
        return Err(LanguageError::WorkspaceChanged);
    }
    if current
        .state
        .lock()
        .map_err(|_| LanguageError::StateUnavailable)?
        .snapshot
        .session_id
        != id
    {
        return Err(LanguageError::StaleSession);
    }
    Ok(current)
}
fn snapshot(session: &Session) -> Result<LanguageSnapshot, LanguageError> {
    let state = session
        .state
        .lock()
        .map_err(|_| LanguageError::StateUnavailable)?;
    let mut snapshot = state.snapshot.clone();
    snapshot.document_versions = state
        .documents
        .iter()
        .map(|(path, doc)| (path.clone(), doc.version))
        .collect();
    Ok(snapshot)
}
fn validate_documents(
    root: &Path,
    documents: Vec<LanguageDocument>,
) -> Result<BTreeMap<String, Arc<LanguageDocument>>, LanguageError> {
    if documents.len() > MAX_DOCUMENTS
        || documents.iter().map(|d| d.text.len()).sum::<usize>() > MAX_TOTAL
    {
        return Err(LanguageError::TooLarge);
    }
    let mut result = BTreeMap::new();
    for doc in documents {
        if doc.text.len() > MAX_DOCUMENT {
            return Err(LanguageError::TooLarge);
        }
        if doc.version <= 0 {
            return Err(LanguageError::InvalidDocument);
        }
        query::checked_path(root, &doc.path)?;
        if result.insert(doc.path.clone(), Arc::new(doc)).is_some() {
            return Err(LanguageError::InvalidDocument);
        }
    }
    Ok(result)
}
fn uri(path: &str) -> String {
    Url::from_file_path(Path::new("/workspace").join(path))
        .expect("absolute sandbox path")
        .to_string()
}
fn configuration() -> Value {
    json!({
        "cargo": {"buildScripts": {"enable": false, "rebuildOnSave": false}, "autoreload": false, "noDeps": true, "sysroot": null},
        "completion": {"autoimport": {"enable": false}},
        "procMacro": {"enable": false}, "checkOnSave": false,
        "cachePriming": {"enable": false}, "numThreads": 2,
        "diagnostics": {"enable": true}, "files": {"watcher": "client"}
    })
}
fn initialization() -> Value {
    json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params": {
        "processId": null, "clientInfo": {"name":"Lyrnova", "version": env!("CARGO_PKG_VERSION")},
        "rootUri":"file:///workspace", "workspaceFolders":[{"uri":"file:///workspace", "name":"workspace"}],
        "capabilities": {"general":{"positionEncodings":["utf-16"]},
            "experimental":{"serverStatusNotification":true},
            "workspace":{"configuration":true, "applyEdit":false},
            "textDocument":{"hover":{"contentFormat":["plaintext"], "dynamicRegistration":false},
                "definition":{"linkSupport":true, "dynamicRegistration":false},
                "completion":{"completionItem":{"snippetSupport":true,"documentationFormat":["plaintext"]}},
                "references":{"dynamicRegistration":false}, "rename":{"dynamicRegistration":false},
                "formatting":{"dynamicRegistration":false},
                "publishDiagnostics":{"versionSupport":true}, "synchronization":{"didSave":false, "dynamicRegistration":false}}},
        "initializationOptions": configuration()
    }})
}
fn parse_diagnostics(
    params: &Value,
    docs: &BTreeMap<String, Arc<LanguageDocument>>,
) -> Option<DocumentDiagnostics> {
    let url = params.get("uri")?.as_str()?;
    let doc = docs.values().find(|doc| uri(&doc.path) == url)?;
    let version = params.get("version")?.as_i64()?;
    if version != i64::from(doc.version) {
        return None;
    }
    let diagnostics = params.get("diagnostics")?.as_array()?;
    let mut items = Vec::new();
    let lines: Vec<_> = doc
        .text
        .split('\n')
        .map(|line| line.trim_end_matches('\r').encode_utf16().count())
        .collect();
    for value in diagnostics.iter().take(MAX_DIAGNOSTICS) {
        let range: Range = serde_json::from_value(value.get("range")?.clone()).ok()?;
        if range.end < range.start {
            return None;
        }
        for pos in [range.start, range.end] {
            let line = lines.get(pos.line as usize)?;
            if pos.character as usize > *line {
                return None;
            }
        }
        let message = value.get("message")?.as_str()?;
        if message.len() > 4096 || message.contains('\0') {
            return None;
        }
        let severity = value.get("severity").and_then(Value::as_u64).unwrap_or(1);
        if !(1..=4).contains(&severity) {
            return None;
        }
        let code = value.get("code").and_then(|c| {
            c.as_str()
                .map(str::to_owned)
                .or_else(|| c.as_i64().map(|n| n.to_string()))
        });
        if code
            .as_ref()
            .is_some_and(|c| c.len() > 128 || c.contains('\0'))
        {
            return None;
        }
        items.push(Diagnostic {
            range,
            severity: severity as u8,
            message: message.into(),
            code,
        });
    }
    Some(DocumentDiagnostics {
        path: doc.path.clone(),
        version: doc.version,
        items,
        truncated: diagnostics.len() > MAX_DIAGNOSTICS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc() -> LanguageDocument {
        LanguageDocument {
            path: "src/a space.rs".into(),
            version: 7,
            text: "let s = \"🦀\";".into(),
        }
    }
    #[test]
    fn closed_files_and_old_sessions_cannot_reintroduce_old_diagnostics() {
        let root = std::env::temp_dir().join(format!("lyrnova-language-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let state = Arc::new(Mutex::new(State {
            sources: vec![],
            configuration: configuration(),
            queries: query::Queries::default(),
            highest_version: 0,
            documents: BTreeMap::new(),
            snapshot: LanguageSnapshot {
                session_id: "session".into(),
                state: "running".into(),
                error: None,
                diagnostics: vec![],
                environment: None,
                analysis_message: None,
                document_versions: BTreeMap::new(),
            },
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let service = LanguageService {
            environments: Mutex::default(),
            session: Mutex::new(Some(Session {
                root: root.clone(),
                state,
                cancel: cancel.clone(),
                worker: None,
            })),
        };
        service.sync(&root, "session", vec![doc()]).unwrap();
        service.sync(&root, "session", vec![]).unwrap();
        assert_eq!(
            service.sync(&root, "session", vec![doc()]),
            Err(LanguageError::InvalidDocument)
        );
        let mut next = doc();
        next.version += 1;
        service.sync(&root, "session", vec![next]).unwrap();
        assert!(matches!(
            service.snapshot(&root, "old"),
            Err(LanguageError::StaleSession)
        ));
        assert!(matches!(
            service.snapshot(&root.join("other"), "session"),
            Err(LanguageError::WorkspaceChanged)
        ));
        service.stop().unwrap();
        assert!(cancel.load(Ordering::Acquire));
        assert!(matches!(
            service.snapshot(&root, "session"),
            Err(LanguageError::StaleSession)
        ));
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn diagnostics_are_scoped_to_open_uri_version_and_utf16_ranges() {
        let doc = doc();
        let docs = [(doc.path.clone(), Arc::new(doc))].into();
        let mut params = json!({"uri":uri("src/a space.rs"), "version":7, "diagnostics":[{"range":{"start":{"line":0,"character":9},"end":{"line":0,"character":11}}, "message":"literal <script>text</script>","severity":2}]});
        assert!(params["uri"].as_str().unwrap().contains("%20"));
        let diagnostics = parse_diagnostics(&params, &docs).unwrap();
        assert_eq!(diagnostics.items[0].range.end.character, 11);
        params["version"] = json!(6);
        assert!(parse_diagnostics(&params, &docs).is_none());
        params["version"] = Value::Null;
        assert!(parse_diagnostics(&params, &docs).is_none());
        params["version"] = json!(7);
        params["uri"] = json!("file:///etc/passwd");
        assert!(parse_diagnostics(&params, &docs).is_none());
        params["uri"] = json!(uri("src/a space.rs"));
        params["diagnostics"][0]["range"]["end"]["character"] = json!(100);
        assert!(parse_diagnostics(&params, &docs).is_none());
        params["diagnostics"] = json!([]);
        assert!(parse_diagnostics(&params, &docs).unwrap().items.is_empty());
    }
    #[test]
    fn bounds_document_snapshots_and_rejects_traversal_symlinks_and_duplicates() {
        let root = std::env::temp_dir().join(format!("lyrnova-language-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for path in [
            "../outside.rs",
            "/outside.rs",
            "src//a.rs",
            "./a.rs",
            ".git/a.rs",
            "src/../a.rs",
            "a\\b.rs",
        ] {
            let mut invalid = doc();
            invalid.path = path.into();
            assert!(matches!(
                validate_documents(&root, vec![invalid]),
                Err(LanguageError::InvalidDocument)
            ));
        }
        std::os::unix::fs::symlink("/tmp", root.join("src")).unwrap();
        assert!(matches!(
            validate_documents(&root, vec![doc()]),
            Err(LanguageError::InvalidDocument)
        ));
        std::fs::remove_file(root.join("src")).unwrap();
        assert!(validate_documents(&root, vec![doc()]).is_ok()); // recovered, missing file is still text-only input
        assert!(matches!(
            validate_documents(&root, vec![doc(), doc()]),
            Err(LanguageError::InvalidDocument)
        ));
        let mut large = doc();
        large.text = "x".repeat(MAX_DOCUMENT + 1);
        assert!(matches!(
            validate_documents(&root, vec![large]),
            Err(LanguageError::TooLarge)
        ));
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn initialization_disables_builds_macros_network_configuration_and_non_utf16() {
        let init = initialization();
        let options = &init["params"]["initializationOptions"];
        assert_eq!(options["cargo"]["buildScripts"]["enable"], false);
        assert_eq!(options["procMacro"]["enable"], false);
        assert_eq!(options["checkOnSave"], false);
        assert_eq!(options["cargo"]["noDeps"], true);
        assert_eq!(
            init["params"]["capabilities"]["general"]["positionEncodings"],
            json!(["utf-16"])
        );
    }
}
