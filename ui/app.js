import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import CssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import HtmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import JsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import TypeScriptWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";
import LyrnovaIcon from "../assets/icons/lyrnova-icon-256.png";

"use strict";

self.MonacoEnvironment = {
  getWorker(_workerId, label) {
    if (label === "json") return new JsonWorker();
    if (["css", "scss", "less"].includes(label)) return new CssWorker();
    if (["html", "handlebars", "razor"].includes(label)) return new HtmlWorker();
    if (["typescript", "javascript"].includes(label)) return new TypeScriptWorker();
    return new EditorWorker();
  },
};

const invoke = window.__TAURI__?.core?.invoke;
const listen = window.__TAURI__?.event?.listen;
const currentWindow = window.__TAURI__?.window?.getCurrentWindow?.();

const appShell = document.querySelector(".app-shell");
const transcript = document.querySelector("#transcript");
const composer = document.querySelector("#composer");
const prompt = document.querySelector("#prompt");
const terminal = document.querySelector("#terminal");
const terminalOutput = document.querySelector("#terminal-output");
const terminalForm = document.querySelector("#terminal-form");
const terminalInput = document.querySelector("#terminal-input");
const palette = document.querySelector("#command-palette");
const paletteInput = document.querySelector("#palette-input");
const agentStatus = document.querySelector("#agent-status");
const srStatus = document.querySelector("#sr-status");
const accountDialog = document.querySelector("#account-dialog");
const integrationNote = document.querySelector("#integration-note");
const profileAvatar = document.querySelector("#profile-avatar");
const profileAccountName = document.querySelector("#profile-account-name");
const profileAccountPlan = document.querySelector("#profile-account-plan");
const accountTitle = document.querySelector("#account-title");
const accountCopy = document.querySelector("#account-copy");
const accountPrivacyTitle = document.querySelector("#account-privacy-title");
const accountPrivacyCopy = document.querySelector("#account-privacy-copy");
const loginOptions = document.querySelector("#login-options");
const logoutButton = document.querySelector("#logout-button");
const projectDialog = document.querySelector("#project-dialog");
const projectForm = document.querySelector("#project-form");
const newProjectName = document.querySelector("#new-project-name");
const newProjectGit = document.querySelector("#new-project-git");
const projectCreateNote = document.querySelector("#project-create-note");
const projectSubmit = projectForm.querySelector('[type="submit"]');
const pluginList = document.querySelector("#plugin-list");
const pluginInstallStatus = document.querySelector("#plugin-install-status");
const trustedPluginList = document.querySelector("#trusted-plugin-list");
const pluginReviewDialog = document.querySelector("#plugin-review-dialog");
const pluginReviewForm = document.querySelector("#plugin-review-form");
const pluginReviewTitle = document.querySelector("#plugin-review-title");
const pluginReviewDescription = document.querySelector("#plugin-review-description");
const pluginReviewMetadata = document.querySelector("#plugin-review-metadata");
const pluginPermissionList = document.querySelector("#plugin-permission-list");
const pluginReviewNote = document.querySelector("#plugin-review-note");
const pluginReviewConfirm = document.querySelector("#plugin-review-confirm");
const pluginRemoveDialog = document.querySelector("#plugin-remove-dialog");
const pluginRemoveForm = document.querySelector("#plugin-remove-form");
const pluginRemoveTitle = document.querySelector("#plugin-remove-title");
const pluginRemoveDescription = document.querySelector("#plugin-remove-description");
const pluginRemoveNote = document.querySelector("#plugin-remove-note");
const pluginRemoveConfirm = document.querySelector("#plugin-remove-confirm");
const conversation = document.querySelector(".conversation");
const editorWorkspace = document.querySelector("#editor-workspace");
const editorTabs = document.querySelector("#editor-tabs");
const editorBreadcrumb = document.querySelector("#editor-breadcrumb");
const sourceEditor = document.querySelector("#source-editor");
const editorEmptyState = document.querySelector("#editor-empty-state");
const editorEmptyIcon = document.querySelector("#editor-empty-icon");
const settingsWorkspace = document.querySelector("#settings-workspace");
const appFontSizeValue = document.querySelector("#app-font-size-value");
const editorFontSizeValue = document.querySelector("#editor-font-size-value");
const terminalFontSizeValue = document.querySelector("#terminal-font-size-value");
const cursorPosition = document.querySelector("#cursor-position");
const languageMode = document.querySelector("#language-mode");
const draftState = document.querySelector("#draft-state");
const fileTree = document.querySelector(".file-tree");
const fileFilter = document.querySelector("#file-filter");
const workspaceRecovery = document.querySelector("#workspace-recovery");
const workspaceRecoveryCopy = document.querySelector("#workspace-recovery-copy");
const gitActivityBadge = document.querySelector("#git-activity-badge");
const gitBranchName = document.querySelector("#git-branch-name");
const gitBranchSummary = document.querySelector("#git-branch-summary");
const projectGitSummary = document.querySelector("#project-git-summary");
const projectName = document.querySelector("#project-name");
const projectMark = document.querySelector("#project-mark");
const contextProjectName = document.querySelector("#context-project-name");
const contextProjectPath = document.querySelector("#context-project-path");
const contextBranch = document.querySelector("#context-branch");
const gitStagedGroup = document.querySelector("#git-staged-group");
const gitStagedCount = document.querySelector("#git-staged-count");
const gitStagedList = document.querySelector("#git-staged-list");
const gitChangesCount = document.querySelector("#git-changes-count");
const gitChangesList = document.querySelector("#git-changes-list");
const gitNote = document.querySelector("#git-note");
const gitCommitMessage = document.querySelector("#git-commit-message");
const gitCommitButton = document.querySelector("#git-commit-button");
const sidebarResizer = document.querySelector("#sidebar-resizer");
const workspaceResizer = document.querySelector("#workspace-resizer");
const taskList = document.querySelector("#task-list");
const tasksStatus = document.querySelector("#tasks-status");
const taskOutput = document.querySelector("#task-output");
const dockContext = document.querySelector("#dock-context");
const cancelTaskButton = document.querySelector("#cancel-task-button");
const taskReviewDialog = document.querySelector("#task-review-dialog");
const taskReviewForm = document.querySelector("#task-review-form");
const taskReviewTitle = document.querySelector("#task-review-title");
const taskReviewDescription = document.querySelector("#task-review-description");
const taskReviewMetadata = document.querySelector("#task-review-metadata");
const taskReviewCommand = document.querySelector("#task-review-command");
const taskRiskNote = document.querySelector("#task-risk-note");
const taskReviewNote = document.querySelector("#task-review-note");
const taskReviewConfirm = document.querySelector("#task-review-confirm");
const approvalSessionRules = document.querySelector("#approval-session-rules");
const approvalAuditList = document.querySelector("#approval-audit-list");
const PLUGIN_PERMISSION_LABELS = Object.freeze({
  workspace_read: ["Ler o workspace", "Acessa arquivos dentro do projeto aberto."],
  workspace_write: ["Alterar o workspace", "Modifica arquivos dentro do projeto aberto."],
  process_spawn: ["Iniciar processos", "Executa o entrypoint e ferramentas declaradas."],
  network_access: ["Acessar a rede", "Pode se comunicar com serviços externos."],
  secret_storage: ["Armazenar segredos", "Usa o cofre local de credenciais."],
  request_approval: ["Solicitar aprovações", "Pode pedir autorização para ações sensíveis."],
});
const DEFAULT_IDE_SETTINGS = Object.freeze({
  appTheme: "system",
  appFontSize: 16,
  interfaceDensity: "comfortable",
  reduceMotion: false,
  editorFontSize: 12,
  editorFontFamily: "system",
  tabSize: 2,
  wordWrap: "off",
  renderWhitespace: "selection",
  minimap: true,
  fontLigatures: true,
  smoothScrolling: true,
  terminalFontSize: 11,
  confirmDirtyClose: true,
});
editorEmptyIcon.src = LyrnovaIcon;
let previousFocus = null;
let constrainPanelWidths = () => {};
let projectCreationRunning = false;
let gitMutationRunning = false;
let currentGitStatus = null;
let currentPluginCatalog = [];
let currentTrustedPluginCatalog = [];
let pendingPluginReview = null;
let pendingPluginRemoval = null;
let pluginMutationRunning = false;
let currentTasks = [];
let pendingTaskReview = null;
let runningTask = null;
let dockView = "terminal";
let currentApprovalState = { sessionRules: [], audit: [] };
let recentProcessAudits = [];

const documentFixtures = new Map([
  ["src-tauri/src/backend.rs", `use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BackendKind {
    Mock,
    CodexAppServer,
}

pub struct CodexAppServerAdapter {
    next_request_id: u64,
    pending: BTreeMap<RequestId, PendingMethod>,
}`],
  ["src-tauri/src/protocol.rs", `use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ApprovalKind {
    Command,
    FileWrite,
    Network,
    ExternalPath,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub thread_id: Option<String>,
    pub active_turn_id: Option<String>,
}`],
  ["ui/index.html", `<!doctype html>
<html lang="pt-BR">
  <head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Lyrnova</title>
    <link rel="stylesheet" href="styles.css">
    <script src="app.js" defer></script>
  </head>
  <body>
    <div class="app-shell">
      <!-- workspace local do Lyrnova -->
    </div>
  </body>
</html>`],
  ["ui/styles.css", `:root {
  color-scheme: dark;
  --bg: #080b18;
  --surface: #111525;
  --text: #f2f3f8;
  --accent: #a979f0;
  --cyan: #6bcde7;
}

.app-shell {
  display: grid;
  grid-template-columns: 264px minmax(0, 1fr);
  height: 100vh;
}`],
  ["ui/app.js", `"use strict";

const appShell = document.querySelector(".app-shell");
const transcript = document.querySelector("#transcript");

function announce(message) {
  srStatus.textContent = "";
  window.requestAnimationFrame(() => {
    srStatus.textContent = message;
  });
}`],
  ["Cargo.toml", `[workspace]
members = ["src-tauri"]
resolver = "2"

[workspace.package]
edition = "2024"
license = "GPL-3.0-only"
rust-version = "1.85"`],
  ["README.md", `# Lyrnova

Lyrnova é um IDE desktop comunitário e extensível. Ele reúne projetos,
Explorer, editor, Git, terminal e plugins opcionais em uma interface própria.`],
]);

const savedDocuments = new Map(documentFixtures);
const draftDocuments = new Map(documentFixtures);
const documentRevisions = new Map();
const openDocuments = ["src-tauri/src/backend.rs"];
let activeDocument = openDocuments[0];
let workspaceEntries = [];
const collapsedDirectories = new Set();
let selectedWorkspacePath = null;
const workspaceRecoveries = [];
let workspaceSearchSequence = 0;
let workspaceSearchTimer = null;
let codeEditor = null;
const editorModels = new Map();
let activeAgentThreadId = null;
let agentTurnRunning = false;
let aiProviderEnabled = false;
let activeAiProvider = null;
let agentEventsBound = false;
let ideSettings = readIdeSettings();
const streamedAgentMessages = new Map();
const pendingApprovalCards = new Map();
let currentProject = null;

const narrowWorkspace = window.matchMedia("(max-width: 900px)");
const systemDarkTheme = window.matchMedia("(prefers-color-scheme: dark)");

function announce(message) {
  srStatus.textContent = "";
  window.requestAnimationFrame(() => { srStatus.textContent = message; });
}

function clampInteger(value, minimum, maximum, fallback) {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? Math.min(Math.max(parsed, minimum), maximum) : fallback;
}

function sanitizeIdeSettings(value = {}) {
  const appThemes = ["system", "dark", "light", "high-contrast"];
  const interfaceDensities = ["comfortable", "compact"];
  const fontFamilies = ["system", "jetbrains", "fira", "consolas"];
  const wordWrapModes = ["off", "on", "bounded"];
  const whitespaceModes = ["selection", "all", "none"];
  return {
    appTheme: appThemes.includes(value.appTheme) ? value.appTheme : DEFAULT_IDE_SETTINGS.appTheme,
    appFontSize: clampInteger(value.appFontSize, 13, 20, DEFAULT_IDE_SETTINGS.appFontSize),
    interfaceDensity: interfaceDensities.includes(value.interfaceDensity) ? value.interfaceDensity : DEFAULT_IDE_SETTINGS.interfaceDensity,
    reduceMotion: typeof value.reduceMotion === "boolean" ? value.reduceMotion : DEFAULT_IDE_SETTINGS.reduceMotion,
    editorFontSize: clampInteger(value.editorFontSize, 10, 28, DEFAULT_IDE_SETTINGS.editorFontSize),
    editorFontFamily: fontFamilies.includes(value.editorFontFamily) ? value.editorFontFamily : DEFAULT_IDE_SETTINGS.editorFontFamily,
    tabSize: [2, 4, 8].includes(Number(value.tabSize)) ? Number(value.tabSize) : DEFAULT_IDE_SETTINGS.tabSize,
    wordWrap: wordWrapModes.includes(value.wordWrap) ? value.wordWrap : DEFAULT_IDE_SETTINGS.wordWrap,
    renderWhitespace: whitespaceModes.includes(value.renderWhitespace) ? value.renderWhitespace : DEFAULT_IDE_SETTINGS.renderWhitespace,
    minimap: typeof value.minimap === "boolean" ? value.minimap : DEFAULT_IDE_SETTINGS.minimap,
    fontLigatures: typeof value.fontLigatures === "boolean" ? value.fontLigatures : DEFAULT_IDE_SETTINGS.fontLigatures,
    smoothScrolling: typeof value.smoothScrolling === "boolean" ? value.smoothScrolling : DEFAULT_IDE_SETTINGS.smoothScrolling,
    terminalFontSize: clampInteger(value.terminalFontSize, 9, 22, DEFAULT_IDE_SETTINGS.terminalFontSize),
    confirmDirtyClose: typeof value.confirmDirtyClose === "boolean" ? value.confirmDirtyClose : DEFAULT_IDE_SETTINGS.confirmDirtyClose,
  };
}

function readIdeSettings() {
  try {
    return sanitizeIdeSettings(JSON.parse(localStorage.getItem("lyrnova.ideSettings") || "{}"));
  } catch {
    return { ...DEFAULT_IDE_SETTINGS };
  }
}

function editorFontFamilyCss(value) {
  return ({
    system: 'ui-monospace, "SFMono-Regular", Consolas, monospace',
    jetbrains: '"JetBrains Mono", ui-monospace, monospace',
    fira: '"Fira Code", ui-monospace, monospace',
    consolas: 'Consolas, "Courier New", monospace',
  })[value];
}

function renderSettingsControls() {
  appFontSizeValue.textContent = `${ideSettings.appFontSize} px`;
  editorFontSizeValue.textContent = `${ideSettings.editorFontSize} px`;
  terminalFontSizeValue.textContent = `${ideSettings.terminalFontSize} px`;
  document.querySelectorAll("[data-setting]").forEach((control) => {
    const key = control.dataset.setting;
    if (!(key in ideSettings)) return;
    if (control.type === "checkbox") control.checked = ideSettings[key];
    else control.value = String(ideSettings[key]);
  });
}

function applyIdeSettings(persist = true) {
  ideSettings = sanitizeIdeSettings(ideSettings);
  const appTheme = ideSettings.appTheme === "system"
    ? (systemDarkTheme.matches ? "dark" : "light")
    : ideSettings.appTheme;
  document.documentElement.dataset.appTheme = appTheme;
  appShell.dataset.appTheme = appTheme;
  appShell.dataset.interfaceDensity = ideSettings.interfaceDensity;
  appShell.dataset.reduceMotion = String(ideSettings.reduceMotion);
  document.documentElement.style.fontSize = `${ideSettings.appFontSize}px`;
  monaco.editor.setTheme(appTheme === "light" ? "lyrnova-light" : (appTheme === "high-contrast" ? "lyrnova-high-contrast" : "lyrnova-dark"));
  codeEditor?.updateOptions({
    fontFamily: editorFontFamilyCss(ideSettings.editorFontFamily),
    fontLigatures: ideSettings.fontLigatures,
    fontSize: ideSettings.editorFontSize,
    minimap: { enabled: ideSettings.minimap, maxColumn: 90, renderCharacters: false },
    renderWhitespace: ideSettings.renderWhitespace,
    smoothScrolling: ideSettings.smoothScrolling,
    tabSize: ideSettings.tabSize,
    wordWrap: ideSettings.wordWrap,
    wordWrapColumn: 120,
  });
  document.documentElement.style.setProperty("--terminal-font-size", `${ideSettings.terminalFontSize}px`);
  renderSettingsControls();
  if (persist) {
    try { localStorage.setItem("lyrnova.ideSettings", JSON.stringify(ideSettings)); } catch { /* preferência não persistente */ }
  }
}

function updateIdeSetting(key, value) {
  if (!(key in DEFAULT_IDE_SETTINGS)) return;
  ideSettings = { ...ideSettings, [key]: value };
  applyIdeSettings();
  announce("Configuração aplicada");
}

function formatBytes(value) {
  const bytes = Number(value) || 0;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
}

function pluginPermissionCopy(permission) {
  return PLUGIN_PERMISSION_LABELS[permission] ?? [permission.replaceAll("_", " "), "Permissão declarada pelo manifesto."];
}

function setPluginInstallStatus(message = "", tone = "info") {
  pluginInstallStatus.hidden = !message;
  pluginInstallStatus.textContent = message;
  pluginInstallStatus.dataset.tone = tone;
}

function pluginFlowErrorMessage(error) {
  let detail = error;
  if (typeof detail === "string") {
    try { detail = JSON.parse(detail); } catch { return "Não foi possível concluir a operação com o plugin."; }
  }
  const code = detail?.error?.code ?? detail?.code ?? detail?.domain;
  return ({
    descriptor_unavailable: "O descritor sidecar <arquivo>.json não foi encontrado.",
    invalid_descriptor: "O descritor do pacote é inválido.",
    checksum_mismatch: "O SHA-256 do pacote não corresponde ao descritor.",
    package_too_large: "O pacote excede o limite de tamanho permitido.",
    invalid_archive: "O arquivo não é um pacote TAR/Zstandard válido.",
    archive_stream_too_large: "O conteúdo descomprimido excede o limite seguro.",
    unsafe_entry_path: "O pacote contém um caminho inseguro.",
    unsupported_entry_type: "O pacote contém links ou tipos de arquivo não permitidos.",
    invalid_manifest: "O manifesto do plugin é inválido ou incompatível.",
    permission_approval_required: "Todas as permissões declaradas precisam ser aceitas.",
    already_installed: "Esta versão do plugin já está instalada.",
    content_integrity_mismatch: "A integridade do conteúdo instalado não pôde ser confirmada.",
    invalid_installed_package: "O pacote instalado falhou na revalidação de segurança.",
    external_package_removal_failed: "Não foi possível mover o pacote para a quarentena de remoção.",
    external_package_rollback_failed: "A remoção falhou e não pôde ser revertida. Plugins externos foram desativados por segurança.",
    external_runtime_unsupported: "Esta plataforma ainda não possui um sandbox para plugins externos.",
    sandbox_unavailable: "O sandbox Bubblewrap não está disponível. O plugin permaneceu desabilitado.",
    runtime_workspace_unavailable: "Este plugin precisa de um workspace ativo para iniciar.",
    runtime_start_failed: "O runtime isolado não pôde ser iniciado. O plugin permaneceu desabilitado.",
    runtime_stop_failed: "O processo do plugin não pôde ser encerrado com segurança.",
    invalid_catalog: "O catálogo curado falhou na validação de segurança.",
    unknown_plugin: "O plugin não existe no catálogo curado desta versão do Lyrnova.",
    download_url_denied: "O destino do download não pertence à allowlist de releases do GitHub.",
    download_failed: "Não foi possível baixar o pacote da release confiável.",
    download_too_large: "O download excede o limite de tamanho permitido.",
    no_trusted_catalog_keys: "A atualização remota está bloqueada até uma chave pública raiz oficial ser provisionada.",
    catalog_signature_invalid: "A assinatura raiz do catálogo não atingiu o limiar confiável.",
    publisher_signature_invalid: "A assinatura do publisher é inválida, ausente ou foi revogada.",
    catalog_expired: "O catálogo remoto expirou e foi recusado.",
    catalog_rollback: "A atualização foi recusada porque repete ou reduz uma versão confiável.",
    unknown_session: "Esta revisão expirou. Selecione o pacote novamente.",
  })[code] ?? "Não foi possível concluir a operação com o plugin.";
}

function createPluginTag(text, tone = "neutral") {
  const tag = document.createElement("span");
  tag.className = "plugin-tag";
  tag.dataset.tone = tone;
  tag.textContent = text;
  return tag;
}

function renderPluginCatalog(plugins = currentPluginCatalog) {
  currentPluginCatalog = Array.isArray(plugins) ? plugins : [];
  pluginList.replaceChildren();
  if (!currentPluginCatalog.length) {
    const empty = document.createElement("p");
    empty.className = "plugin-list-empty";
    empty.textContent = "Nenhum plugin válido foi encontrado.";
    pluginList.append(empty);
    return;
  }

  currentPluginCatalog.forEach((plugin) => {
    const card = document.createElement("article");
    card.className = "plugin-card";
    card.dataset.enabled = String(Boolean(plugin.enabled));

    const heading = document.createElement("div");
    heading.className = "plugin-card-heading";
    const mark = document.createElement("span");
    mark.className = "plugin-card-mark";
    mark.textContent = plugin.kind === "language" ? "{ }" : "◇";
    const identity = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = plugin.name;
    const meta = document.createElement("small");
    meta.textContent = `${plugin.publisher} · ${plugin.version}`;
    identity.append(title, meta);
    const status = createPluginTag(
      plugin.enabled ? "Ativo" : (plugin.installed ? "Desabilitado" : "Disponível"),
      plugin.enabled ? "success" : (plugin.installed ? "warning" : "neutral"),
    );
    heading.append(mark, identity, status);

    const description = document.createElement("p");
    description.textContent = plugin.description;
    const tags = document.createElement("div");
    tags.className = "plugin-card-tags";
    tags.append(createPluginTag(plugin.bundled ? "Embutido" : "Externo", plugin.bundled ? "neutral" : "accent"));
    if (!plugin.bundled) {
      tags.append(createPluginTag(plugin.publisherVerified ? "Publisher autenticado" : "Pacote local não autenticado", plugin.publisherVerified ? "success" : "warning"));
    }
    plugin.permissions.forEach((permission) => tags.append(createPluginTag(pluginPermissionCopy(permission)[0])));

    const footer = document.createElement("div");
    footer.className = "plugin-card-footer";
    const authority = document.createElement("small");
    authority.textContent = plugin.requiresPermissionReview ? "Revisão de permissões necessária" : "Permissões revisadas";
    footer.append(authority);
    const actions = document.createElement("div");
    actions.className = "plugin-card-actions";
    if (plugin.installed && !plugin.bundled) {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "danger-button subtle-danger";
      remove.dataset.pluginRemove = plugin.id;
      remove.textContent = "Remover";
      actions.append(remove);
    }
    if (plugin.installed) {
      const toggle = document.createElement("button");
      toggle.type = "button";
      toggle.className = "secondary-button";
      toggle.dataset.pluginToggle = plugin.id;
      toggle.dataset.enable = String(!plugin.enabled);
      toggle.textContent = plugin.enabled ? "Desabilitar" : "Ativar";
      actions.append(toggle);
    }
    if (actions.childElementCount) footer.append(actions);
    card.append(heading, description, tags, footer);
    pluginList.append(card);
  });
}

function renderTrustedPluginCatalog(entries = currentTrustedPluginCatalog) {
  currentTrustedPluginCatalog = Array.isArray(entries) ? entries : [];
  trustedPluginList.replaceChildren();
  if (!currentTrustedPluginCatalog.length) {
    const empty = document.createElement("p");
    empty.className = "plugin-list-empty";
    empty.textContent = "Nenhum pacote foi publicado no catálogo curado desta versão.";
    trustedPluginList.append(empty);
    return;
  }

  currentTrustedPluginCatalog.forEach((entry) => {
    const plugin = entry.manifest;
    const installedVersion = entry.installedVersion;
    const exactVersionInstalled = installedVersion === plugin.version;
    const card = document.createElement("article");
    card.className = "plugin-card trusted-plugin-card";

    const heading = document.createElement("div");
    heading.className = "plugin-card-heading";
    const mark = document.createElement("span");
    mark.className = "plugin-card-mark";
    mark.textContent = plugin.kind === "language" ? "{ }" : "◇";
    const identity = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = plugin.name;
    const meta = document.createElement("small");
    meta.textContent = `${plugin.publisher} · ${plugin.version}`;
    identity.append(title, meta);
    heading.append(mark, identity, createPluginTag(exactVersionInstalled ? "Instalado" : "Curado", exactVersionInstalled ? "success" : "accent"));

    const description = document.createElement("p");
    description.textContent = plugin.description;
    const tags = document.createElement("div");
    tags.className = "plugin-card-tags";
    tags.append(createPluginTag("Assinatura Ed25519", "success"));
    plugin.permissions.forEach((permission) => tags.append(createPluginTag(pluginPermissionCopy(permission)[0])));
    const footer = document.createElement("div");
    footer.className = "plugin-card-footer";
    const integrity = document.createElement("small");
    integrity.className = "monospace";
    integrity.textContent = `SHA-256 ${entry.descriptor.sha256.slice(0, 12)}… · chave ${entry.publisherKeyId.slice(0, 12)}…`;
    const download = document.createElement("button");
    download.type = "button";
    download.className = "accent-button";
    download.dataset.pluginDownload = plugin.id;
    download.disabled = !entry.downloadAvailable;
    download.textContent = exactVersionInstalled
      ? "Instalado"
      : (installedVersion ? (entry.downloadAvailable ? "Baixar atualização" : "Versão mais nova instalada") : "Baixar e revisar");
    footer.append(integrity, download);
    card.append(heading, description, tags, footer);
    trustedPluginList.append(card);
  });
}

function appendReviewMetadata(label, value, monospace = false) {
  const term = document.createElement("dt");
  term.textContent = label;
  const detail = document.createElement("dd");
  detail.textContent = value;
  if (monospace) detail.className = "monospace";
  pluginReviewMetadata.append(term, detail);
}

function updatePluginReviewConfirmation() {
  const permissions = [...pluginPermissionList.querySelectorAll('input[type="checkbox"]')];
  pluginReviewConfirm.disabled = pluginMutationRunning || permissions.some((input) => !input.checked);
}

function showPluginReview(stage) {
  pendingPluginReview = stage;
  const { manifest, descriptor, packageBytes, entryCount, expandedBytes } = stage.review;
  pluginReviewTitle.textContent = `${manifest.name} ${manifest.version}`;
  pluginReviewDescription.textContent = manifest.description;
  pluginReviewMetadata.replaceChildren();
  appendReviewMetadata("Publisher", manifest.publisher);
  appendReviewMetadata("Licença", manifest.license);
  appendReviewMetadata("Pacote", `${descriptor.asset} · ${formatBytes(packageBytes)}`);
  appendReviewMetadata("Conteúdo", `${entryCount} entradas · ${formatBytes(expandedBytes)}`);
  appendReviewMetadata("SHA-256", descriptor.sha256, true);
  appendReviewMetadata("Repositório", manifest.source.repository, true);
  appendReviewMetadata(
    "Autenticidade",
    stage.review.authentication
      ? `Publisher autenticado · chave ${stage.review.authentication.keyId.slice(0, 16)}…`
      : "Pacote local sem assinatura de publisher",
    Boolean(stage.review.authentication),
  );

  pluginPermissionList.replaceChildren();
  manifest.permissions.forEach((permission) => {
    const [title, copy] = pluginPermissionCopy(permission);
    const label = document.createElement("label");
    label.className = "plugin-permission";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.value = permission;
    checkbox.checked = true;
    const text = document.createElement("span");
    const strong = document.createElement("strong");
    strong.textContent = title;
    const small = document.createElement("small");
    small.textContent = copy;
    text.append(strong, small);
    label.append(checkbox, text);
    pluginPermissionList.append(label);
  });
  pluginReviewNote.hidden = true;
  pluginReviewNote.textContent = "";
  pluginMutationRunning = false;
  pluginReviewConfirm.textContent = "Instalar desabilitado";
  updatePluginReviewConfirmation();
  previousFocus = document.activeElement;
  pluginReviewDialog.showModal();
  pluginReviewConfirm.focus();
}

async function selectPluginPackage() {
  if (!invoke || pluginMutationRunning) return;
  pluginMutationRunning = true;
  setPluginInstallStatus("Validando pacote e preparando revisão…");
  const trigger = document.querySelector('[data-action="select-plugin-package"]');
  if (trigger) trigger.disabled = true;
  try {
    const stage = await invoke("plugin_package_select");
    if (stage) {
      setPluginInstallStatus();
      showPluginReview(stage);
    } else {
      setPluginInstallStatus("Seleção cancelada.");
    }
  } catch (error) {
    setPluginInstallStatus(pluginFlowErrorMessage(error), "error");
    announce("Pacote de plugin recusado");
  } finally {
    pluginMutationRunning = false;
    if (trigger) trigger.disabled = false;
  }
}

async function downloadPluginPackage(pluginId) {
  if (!invoke || pluginMutationRunning) return;
  pluginMutationRunning = true;
  setPluginInstallStatus("Baixando pacote do catálogo curado…");
  const trigger = [...trustedPluginList.querySelectorAll("[data-plugin-download]")]
    .find((button) => button.dataset.pluginDownload === pluginId);
  if (trigger) trigger.disabled = true;
  try {
    const stage = await invoke("plugin_package_download", { pluginId });
    setPluginInstallStatus();
    showPluginReview(stage);
  } catch (error) {
    setPluginInstallStatus(pluginFlowErrorMessage(error), "error");
    announce("Download de plugin recusado");
  } finally {
    pluginMutationRunning = false;
    if (trigger) trigger.disabled = false;
  }
}

async function updatePluginCatalog() {
  if (!invoke || pluginMutationRunning) return;
  pluginMutationRunning = true;
  const trigger = document.querySelector('[data-action="update-plugin-catalog"]');
  if (trigger) trigger.disabled = true;
  setPluginInstallStatus("Baixando e autenticando metadados do catálogo…");
  try {
    const entries = await invoke("plugin_catalog_update");
    renderTrustedPluginCatalog(entries);
    const plugins = await invoke("plugin_list");
    renderPluginCatalog(plugins);
    await refreshAiProviderAvailability();
    await loadTasks();
    setPluginInstallStatus("Catálogo autenticado e atualizado.", "success");
    announce("Catálogo de plugins atualizado");
  } catch (error) {
    setPluginInstallStatus(pluginFlowErrorMessage(error), "error");
    announce("Atualização do catálogo recusada");
  } finally {
    pluginMutationRunning = false;
    if (trigger) trigger.disabled = false;
  }
}

async function cancelPluginReview() {
  if (pluginMutationRunning) return;
  const token = pendingPluginReview?.token;
  pendingPluginReview = null;
  if (pluginReviewDialog.open) pluginReviewDialog.close();
  previousFocus?.focus();
  if (!invoke || !token) return;
  try { await invoke("plugin_package_cancel", { token }); } catch { /* sessão já encerrada */ }
  setPluginInstallStatus("Instalação cancelada; o staging foi removido.");
}

async function confirmPluginInstall() {
  if (!invoke || !pendingPluginReview || pluginMutationRunning) return;
  const approvedPermissions = [...pluginPermissionList.querySelectorAll('input[type="checkbox"]:checked')]
    .map((input) => input.value);
  pluginMutationRunning = true;
  pluginReviewConfirm.disabled = true;
  pluginReviewConfirm.textContent = "Instalando…";
  pluginReviewNote.hidden = true;
  try {
    const plugins = await invoke("plugin_package_confirm", {
      token: pendingPluginReview.token,
      approvedPermissions,
    });
    pendingPluginReview = null;
    pluginReviewDialog.close();
    renderPluginCatalog(plugins);
    renderTrustedPluginCatalog();
    await refreshAiProviderAvailability();
    await loadTasks();
    setPluginInstallStatus("Plugin instalado e revisado. Ele permanece desabilitado até você ativá-lo.", "success");
    announce("Plugin instalado e mantido desabilitado");
  } catch (error) {
    pluginReviewNote.textContent = pluginFlowErrorMessage(error);
    pluginReviewNote.hidden = false;
    announce("Falha ao instalar plugin");
  } finally {
    pluginMutationRunning = false;
    pluginReviewConfirm.textContent = "Instalar desabilitado";
    updatePluginReviewConfirmation();
  }
}

async function setPluginEnabled(pluginId, enabled) {
  if (!invoke || pluginMutationRunning) return;
  pluginMutationRunning = true;
  try {
    const plugins = await invoke("plugin_set_enabled", { pluginId, enabled });
    renderPluginCatalog(plugins);
    renderTrustedPluginCatalog();
    await refreshAiProviderAvailability();
    await loadTasks();
    setPluginInstallStatus(`Plugin ${enabled ? "ativado" : "desabilitado"}.`, "success");
  } catch (error) {
    setPluginInstallStatus(pluginFlowErrorMessage(error), "error");
  } finally {
    pluginMutationRunning = false;
  }
}

function showPluginRemoval(pluginId) {
  if (pluginMutationRunning) return;
  const plugin = currentPluginCatalog.find((entry) => entry.id === pluginId && entry.installed && !entry.bundled);
  if (!plugin) return;
  pendingPluginRemoval = plugin;
  pluginRemoveTitle.textContent = `Remover ${plugin.name}?`;
  pluginRemoveDescription.textContent = `${plugin.publisher} · ${plugin.version}`;
  pluginRemoveNote.hidden = true;
  pluginRemoveNote.textContent = "";
  pluginRemoveConfirm.disabled = false;
  pluginRemoveConfirm.textContent = "Remover plugin";
  previousFocus = document.activeElement;
  pluginRemoveDialog.showModal();
  pluginRemoveConfirm.focus();
}

function cancelPluginRemoval() {
  if (pluginMutationRunning) return;
  pendingPluginRemoval = null;
  if (pluginRemoveDialog.open) pluginRemoveDialog.close();
  previousFocus?.focus();
}

async function confirmPluginRemoval() {
  if (!invoke || !pendingPluginRemoval || pluginMutationRunning) return;
  pluginMutationRunning = true;
  pluginRemoveConfirm.disabled = true;
  pluginRemoveConfirm.textContent = "Removendo…";
  pluginRemoveNote.hidden = true;
  try {
    const removedName = pendingPluginRemoval.name;
    const plugins = await invoke("plugin_uninstall", { pluginId: pendingPluginRemoval.id });
    pendingPluginRemoval = null;
    pluginRemoveDialog.close();
    renderPluginCatalog(plugins);
    renderTrustedPluginCatalog();
    await refreshAiProviderAvailability();
    await loadTasks();
    setPluginInstallStatus(`${removedName} e todas as suas versões locais foram removidos.`, "success");
    announce("Plugin externo removido");
  } catch (error) {
    pluginRemoveNote.textContent = pluginFlowErrorMessage(error);
    pluginRemoveNote.hidden = false;
    announce("Falha ao remover plugin");
  } finally {
    pluginMutationRunning = false;
    pluginRemoveConfirm.disabled = false;
    pluginRemoveConfirm.textContent = "Remover plugin";
  }
}

function registerRustCompletions() {
  const snippets = [
    ["fn", "função", "fn ${1:name}(${2}) -> ${3:Result<()>} {\n  ${0}\n}"],
    ["struct", "estrutura", "struct ${1:Name} {\n  ${2:field}: ${3:String},\n}"],
    ["impl", "bloco impl", "impl ${1:Type} {\n  ${0}\n}"],
    ["match", "expressão match", "match ${1:value} {\n  ${2:pattern} => ${3:result},\n  _ => ${0},\n}"],
    ["test", "teste unitário", "#[test]\nfn ${1:name}() {\n  ${0}\n}"],
    ["println", "macro println!", "println!(\"${1}\"${2});"],
  ];

  monaco.languages.registerCompletionItemProvider("rust", {
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };
      return {
        suggestions: snippets.map(([label, detail, insertText]) => ({
          label,
          detail: `Lyrnova · ${detail}`,
          insertText,
          insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          kind: monaco.languages.CompletionItemKind.Snippet,
          range,
        })),
      };
    },
  });
}

function initializeCodeEditor() {
  monaco.editor.defineTheme("lyrnova-dark", {
    base: "vs-dark",
    inherit: true,
    colors: {
      "editor.background": "#0a0d19",
      "editor.foreground": "#cfd4e4",
      "editorCursor.foreground": "#6bcde7",
      "editor.lineHighlightBackground": "#111525",
      "editorLineNumber.foreground": "#4f5873",
      "editorLineNumber.activeForeground": "#aeb5cb",
      "editor.selectionBackground": "#345d7166",
      "editor.inactiveSelectionBackground": "#293b4c66",
      "editorIndentGuide.background1": "#252b3f",
      "editorIndentGuide.activeBackground1": "#59627f",
      "editorSuggestWidget.background": "#171b2e",
      "editorSuggestWidget.border": "#3d4460",
      "editorSuggestWidget.selectedBackground": "#2b3150",
    },
    rules: [
      { token: "comment", foreground: "69728f", fontStyle: "italic" },
      { token: "keyword", foreground: "bd8df6" },
      { token: "string", foreground: "9fd6b8" },
      { token: "number", foreground: "e5bc60" },
      { token: "type", foreground: "77cce2" },
      { token: "type.identifier", foreground: "77cce2" },
    ],
  });
  monaco.editor.defineTheme("lyrnova-light", {
    base: "vs",
    inherit: true,
    colors: {
      "editor.background": "#fbfaff",
      "editor.foreground": "#303244",
      "editorCursor.foreground": "#7650bf",
      "editor.lineHighlightBackground": "#f1edf8",
      "editorLineNumber.foreground": "#9a96a8",
      "editorLineNumber.activeForeground": "#555366",
      "editor.selectionBackground": "#c9b8e866",
      "editor.inactiveSelectionBackground": "#ded4ee66",
      "editorIndentGuide.background1": "#e2ddeb",
      "editorIndentGuide.activeBackground1": "#aaa1bc",
      "editorSuggestWidget.background": "#ffffff",
      "editorSuggestWidget.border": "#d9d2e5",
      "editorSuggestWidget.selectedBackground": "#eee8f7",
    },
    rules: [
      { token: "comment", foreground: "777487", fontStyle: "italic" },
      { token: "keyword", foreground: "7042b6" },
      { token: "string", foreground: "287858" },
      { token: "number", foreground: "9a6414" },
      { token: "type", foreground: "176c83" },
      { token: "type.identifier", foreground: "176c83" },
    ],
  });
  monaco.editor.defineTheme("lyrnova-high-contrast", {
    base: "hc-black",
    inherit: true,
    colors: {
      "editor.background": "#000000",
      "editor.foreground": "#ffffff",
      "editorCursor.foreground": "#65e7ff",
      "editor.lineHighlightBackground": "#171717",
      "editorLineNumber.foreground": "#b8b8b8",
      "editorLineNumber.activeForeground": "#ffffff",
      "editor.selectionBackground": "#7755aa",
    },
    rules: [
      { token: "comment", foreground: "CFCFCF", fontStyle: "italic" },
      { token: "keyword", foreground: "E2B7FF" },
      { token: "string", foreground: "8DFFC5" },
      { token: "number", foreground: "FFE078" },
      { token: "type", foreground: "79E9FF" },
      { token: "type.identifier", foreground: "79E9FF" },
    ],
  });
  registerRustCompletions();

  codeEditor = monaco.editor.create(sourceEditor, {
    model: null,
    theme: "lyrnova-dark",
    automaticLayout: true,
    accessibilitySupport: "auto",
    autoClosingBrackets: "always",
    autoClosingQuotes: "always",
    bracketPairColorization: { enabled: true },
    cursorBlinking: "smooth",
    detectIndentation: true,
    fontFamily: editorFontFamilyCss(ideSettings.editorFontFamily),
    fontLigatures: ideSettings.fontLigatures,
    fontSize: ideSettings.editorFontSize,
    folding: true,
    formatOnPaste: true,
    glyphMargin: true,
    guides: { bracketPairs: true, indentation: true },
    largeFileOptimizations: true,
    lineDecorationsWidth: 14,
    lineHeight: 20,
    lineNumbersMinChars: 4,
    minimap: { enabled: ideSettings.minimap, maxColumn: 90, renderCharacters: false },
    padding: { top: 10, bottom: 20 },
    parameterHints: { enabled: true },
    quickSuggestions: { comments: false, other: true, strings: true },
    renderWhitespace: ideSettings.renderWhitespace,
    scrollBeyondLastLine: false,
    showFoldingControls: "always",
    smoothScrolling: ideSettings.smoothScrolling,
    stickyScroll: { enabled: true },
    suggestOnTriggerCharacters: true,
    tabSize: ideSettings.tabSize,
    wordWrap: ideSettings.wordWrap,
    wordWrapColumn: 120,
    wordBasedSuggestions: "matchingDocuments",
  });

  codeEditor.onDidChangeModelContent(() => {
    if (!activeDocument || !codeEditor.getModel()) return;
    draftDocuments.set(activeDocument, codeEditor.getValue());
    updateDraftState();
  });
  codeEditor.onDidChangeModel(updateEditorEmptyState);
  codeEditor.onDidChangeCursorPosition(updateCursorPosition);
  codeEditor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => { void saveDocument(); });
  codeEditor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyW, () => {
    if (activeDocument) void closeDocument(activeDocument);
  });
  applyIdeSettings(false);
  updateEditorEmptyState();
}

function toggleInspector(force) {
  const current = appShell.dataset.inspectorOpen === "true";
  const next = typeof force === "boolean" ? force : !current;
  if (next && appShell.dataset.workspaceView === "settings") showWorkspaceView("editor");
  appShell.dataset.inspectorOpen = String(next);
  if (next) appShell.dataset.chatOpen = "false";
  document.querySelectorAll('[data-action="toggle-inspector"]').forEach((button) => button.setAttribute("aria-pressed", String(next)));
  announce(next ? "Inspector aberto" : "Inspector fechado");
}

function toggleTerminal(force) {
  const next = typeof force === "boolean" ? force : terminal.hidden;
  terminal.hidden = !next;
  if (next) {
    if (dockView === "terminal") {
      void startTerminal();
      window.requestAnimationFrame(() => terminalInput.focus());
    }
  }
  document.querySelectorAll('[data-action="toggle-terminal"]').forEach((button) => button.setAttribute("aria-pressed", String(next)));
  announce(next ? "Terminal aberto" : "Terminal fechado");
}

function appendTerminalOutput(data) {
  terminalOutput.textContent += data;
  if (terminalOutput.textContent.length > 250_000) {
    terminalOutput.textContent = terminalOutput.textContent.slice(-200_000);
  }
  terminalOutput.scrollTop = terminalOutput.scrollHeight;
}

async function startTerminal(restart = false) {
  if (!invoke) return;
  if (!currentProject) {
    appendTerminalOutput("\n[Abra ou crie um projeto antes de iniciar o terminal]\n");
    return;
  }
  try {
    if (restart) {
      await invoke("terminal_stop");
      terminalOutput.textContent = "Terminal reiniciado · /bin/bash\n";
    }
    await invoke("terminal_start");
  } catch {
    appendTerminalOutput("\n[Não foi possível iniciar o terminal local]\n");
  }
}

async function bindTerminalOutput() {
  if (!listen) return;
  try {
    await listen("terminal-output", ({ payload }) => appendTerminalOutput(payload.data));
  } catch {
    appendTerminalOutput("\n[Streaming do terminal indisponível]\n");
  }
}

function setDockView(view) {
  dockView = view === "tasks" ? "tasks" : "terminal";
  const showingTasks = dockView === "tasks";
  terminalOutput.hidden = showingTasks;
  terminalForm.hidden = showingTasks;
  taskOutput.hidden = !showingTasks;
  dockContext.textContent = showingTasks ? (runningTask?.label || "Saída de Tasks") : "/bin/bash";
  cancelTaskButton.hidden = !showingTasks || !runningTask;
  document.querySelectorAll("[data-dock-view]").forEach((button) => {
    button.classList.toggle("active", button.dataset.dockView === dockView);
  });
  if (!showingTasks) void startTerminal();
}

function appendTaskOutput(data) {
  taskOutput.textContent += data;
  if (taskOutput.textContent.length > 1_100_000) {
    taskOutput.textContent = taskOutput.textContent.slice(-1_000_000);
  }
  taskOutput.scrollTop = taskOutput.scrollHeight;
}

async function bindTaskOutput() {
  if (!listen) return;
  try {
    await listen("task-output", ({ payload }) => {
      if (!runningTask || payload.processId !== runningTask.processId) return;
      appendTaskOutput(payload.data);
    });
  } catch {
    appendTaskOutput("\n[Streaming de Tasks indisponível]\n");
  }
}

async function bindProcessAudit() {
  if (!listen) return;
  try {
    await listen("process-audit", ({ payload }) => {
      if (!payload?.eventId || recentProcessAudits.some((entry) => entry.eventId === payload.eventId)) return;
      recentProcessAudits.unshift({ ...payload, observedAtMs: Date.now() });
      recentProcessAudits = recentProcessAudits.slice(0, 100);
      renderApprovalState(currentApprovalState);
    });
  } catch {
    /* Auditoria de processo é complementar; a fronteira Rust continua ativa. */
  }
}

function taskAccessLabel(task) {
  if (task.access === "workspace_write") return "Escrita";
  return task.network ? "Rede" : "Somente leitura";
}

function renderTasks(items = currentTasks) {
  currentTasks = Array.isArray(items) ? items : [];
  taskList.replaceChildren();
  if (!currentTasks.length) {
    const empty = document.createElement("p");
    empty.className = "tasks-status";
    empty.textContent = "Nenhum plugin ativo oferece Tasks para este workspace.";
    taskList.append(empty);
    return;
  }
  currentTasks.forEach((task) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "task-item";
    button.dataset.taskPlugin = task.pluginId;
    button.dataset.taskId = task.taskId;
    const mark = document.createElement("span");
    mark.className = "task-item-mark";
    mark.textContent = "▷";
    const copy = document.createElement("span");
    copy.className = "task-item-copy";
    const title = document.createElement("strong");
    title.textContent = task.label;
    const detail = document.createElement("small");
    detail.textContent = task.detail ? `${task.pluginName} · ${task.detail}` : task.pluginName;
    copy.append(title, detail);
    const risk = document.createElement("span");
    risk.className = "task-item-risk";
    risk.textContent = taskAccessLabel(task);
    button.append(mark, copy, risk);
    taskList.append(button);
  });
}

function taskFlowErrorMessage(error) {
  const domain = error?.domain;
  const nested = error?.error?.domain || error?.error?.code || error?.error;
  if (domain === "no_workspace") return "Abra um projeto antes de executar Tasks.";
  if (nested === "authorization_changed") return "As permissões do plugin mudaram; revise a Task novamente.";
  if (nested === "permission_denied") return "A Task pede uma autoridade que o plugin não possui.";
  if (domain === "runtime") return "O runtime do plugin recusou ou interrompeu a operação.";
  if (domain === "plugin") return "O plugin não está mais instalado, ativo ou autorizado.";
  return "A Task foi recusada pela política do Lyrnova.";
}

async function loadTasks() {
  if (!invoke) {
    tasksStatus.textContent = "Tasks ficam disponíveis no aplicativo desktop.";
    renderTasks([]);
    return;
  }
  tasksStatus.removeAttribute("data-state");
  tasksStatus.textContent = "Consultando plugins ativos…";
  try {
    const catalog = await invoke("task_list");
    renderTasks(catalog.items);
    const strong = catalog.sandbox?.isolatedNetwork === "strong";
    tasksStatus.dataset.state = strong ? "strong" : "error";
    tasksStatus.textContent = strong
      ? `${catalog.items.length} ${catalog.items.length === 1 ? "Task disponível" : "Tasks disponíveis"} · sandbox forte ativo`
      : "Sandbox forte indisponível; execuções protegidas falharão fechadas.";
  } catch (error) {
    renderTasks([]);
    tasksStatus.dataset.state = "error";
    tasksStatus.textContent = taskFlowErrorMessage(error);
  }
}

function appendTaskReviewMetadata(label, value) {
  const term = document.createElement("dt");
  term.textContent = label;
  const detail = document.createElement("dd");
  detail.textContent = value;
  detail.title = value;
  taskReviewMetadata.append(term, detail);
}

async function showTaskReview(pluginId, taskId) {
  if (!invoke || runningTask) {
    if (runningTask) announce("Cancele ou aguarde a Task atual antes de iniciar outra");
    return;
  }
  tasksStatus.textContent = "Preparando revisão da Task…";
  try {
    const review = await invoke("task_review", { pluginId, taskId });
    pendingTaskReview = review;
    taskReviewTitle.textContent = review.label;
    taskReviewDescription.textContent = review.detail || `Task fornecida por ${review.pluginName}.`;
    taskReviewMetadata.replaceChildren();
    appendTaskReviewMetadata("Plugin", `${review.pluginName} · ${review.pluginId}`);
    appendTaskReviewMetadata("Diretório", review.process.cwd);
    appendTaskReviewMetadata("Acesso", review.process.access === "workspace_write" ? "Escrita limitada ao workspace" : "Workspace somente leitura");
    appendTaskReviewMetadata("Rede", review.process.network ? "Permitida pelo grant do plugin" : "Negada");
    appendTaskReviewMetadata("Sandbox", review.process.sandbox === "strong" ? "Forte" : "Indisponível");
    appendTaskReviewMetadata("Timeout", `${Math.round(review.process.timeoutMs / 1000)} s`);
    if (review.process.environmentKeys?.length) {
      appendTaskReviewMetadata("Dados redigidos", `Valores de environment ocultos (${review.process.environmentKeys.join(", ")})`);
    } else {
      appendTaskReviewMetadata("Dados redigidos", "Nenhum");
    }
    appendTaskReviewMetadata("Validade", `${Math.round(review.process.expiresInMs / 60_000)} min · uso único`);
    appendTaskReviewMetadata("Hash da ação", review.process.actionSha256);
    taskReviewCommand.textContent = review.process.command;
    taskRiskNote.dataset.risk = review.process.risk;
    taskRiskNote.textContent = review.process.risk === "destructive"
      ? "O comando contém uma operação potencialmente destrutiva. Confira cuidadosamente antes de executar."
      : (review.process.risk === "approval_required"
        ? "Esta execução usa shell, escrita ou rede e exige sua confirmação explícita."
        : "Execução somente leitura. O comando ainda será isolado e auditado.");
    taskReviewNote.hidden = true;
    taskReviewConfirm.disabled = false;
    taskReviewConfirm.className = review.process.risk === "destructive" ? "danger-button" : "accent-button";
    taskReviewConfirm.textContent = review.process.risk === "destructive" ? "Executar ação destrutiva" : "Executar task";
    previousFocus = document.activeElement;
    taskReviewDialog.showModal();
    taskReviewConfirm.focus();
    void loadTasks();
  } catch (error) {
    tasksStatus.dataset.state = "error";
    tasksStatus.textContent = taskFlowErrorMessage(error);
  }
}

async function cancelTaskReview() {
  const token = pendingTaskReview?.process.reviewToken;
  pendingTaskReview = null;
  if (taskReviewDialog.open) taskReviewDialog.close();
  previousFocus?.focus();
  if (!invoke || !token) return;
  try { await invoke("task_review_discard", { reviewToken: token }); } catch { /* token expirado ou já consumido */ }
}

async function executeReviewedTask() {
  if (!invoke || !pendingTaskReview || runningTask) return;
  const review = pendingTaskReview;
  pendingTaskReview = null;
  taskReviewConfirm.disabled = true;
  taskReviewDialog.close();
  runningTask = {
    processId: review.process.processId,
    label: review.label,
  };
  taskOutput.textContent = `Task: ${review.label}\nPlugin: ${review.pluginName}\nComando: ${review.process.command}\n\n`;
  terminal.hidden = false;
  setDockView("tasks");
  announce(`Executando ${review.label}`);
  try {
    const result = await invoke("task_execute", {
      reviewToken: review.process.reviewToken,
      actionSha256: review.process.actionSha256,
    });
    const code = result.exitCode === null ? "sem código" : `código ${result.exitCode}`;
    appendTaskOutput(`\n[${result.outcome} · ${code} · ${result.durationMs} ms${result.stdoutTruncated || result.stderrTruncated ? " · saída truncada" : ""}]\n`);
    announce(result.outcome === "exited" && result.exitCode === 0 ? "Task concluída" : "Task encerrada");
  } catch (error) {
    appendTaskOutput(`\n[${taskFlowErrorMessage(error)}]\n`);
    announce("Task recusada ou interrompida");
  } finally {
    runningTask = null;
    cancelTaskButton.hidden = true;
    dockContext.textContent = "Saída de Tasks";
  }
}

async function cancelRunningTask() {
  if (!invoke || !runningTask) return;
  cancelTaskButton.disabled = true;
  try {
    await invoke("task_cancel", { processId: runningTask.processId });
    appendTaskOutput("\n[Cancelamento solicitado]\n");
  } catch {
    appendTaskOutput("\n[A Task já não está em execução]\n");
  } finally {
    cancelTaskButton.disabled = false;
  }
}

function switchActivity(panel) {
  if (panel === "agent" && !aiProviderEnabled) {
    announce("Instale e ative um plugin de IA para abrir conversas");
    return;
  }
  appShell.dataset.activityPanel = panel;
  document.querySelectorAll("[data-activity]").forEach((button) => {
    const active = button.dataset.activity === panel;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  if (panel === "settings") {
    showWorkspaceView("settings");
  } else if (panel === "agent") {
    showWorkspaceView("agent");
  } else {
    showWorkspaceView("editor");
    if (narrowWorkspace.matches) appShell.dataset.sidebarOpen = "true";
  }
  if (panel === "git") void loadGitStatus();
  if (panel === "tasks") void loadTasks();
  announce(({ explorer: "Explorer aberto", git: "Controle de código-fonte aberto", tasks: "Tasks abertas", agent: "Conversas abertas", settings: "Configurações abertas" })[panel]);
}

function clamp(value, minimum, maximum) {
  return Math.min(Math.max(value, minimum), maximum);
}

function bindPanelResizers() {
  const restore = (key, fallback) => {
    const value = Number.parseFloat(localStorage.getItem(key));
    return Number.isFinite(value) ? value : fallback;
  };

  const setSidebarWidth = (requested) => {
    if (narrowWorkspace.matches) return;
    const activityWidth = document.querySelector(".activity-bar").getBoundingClientRect().width;
    const maximum = Math.max(180, Math.min(420, window.innerWidth - activityWidth - 620));
    const width = clamp(requested, 180, maximum);
    appShell.style.setProperty("--sidebar-width", `${width}px`);
    sidebarResizer.setAttribute("aria-valuemin", "180");
    sidebarResizer.setAttribute("aria-valuemax", String(Math.round(maximum)));
    sidebarResizer.setAttribute("aria-valuenow", String(Math.round(width)));
    localStorage.setItem("lyrnova.sidebarWidth", String(width));
  };

  const setRightPanelWidth = (requested) => {
    if (narrowWorkspace.matches) return;
    const body = document.querySelector(".workspace-body");
    const minimum = Math.min(280, Math.max(220, body.clientWidth * .35));
    const maximum = Math.max(minimum, body.clientWidth - 320);
    const width = clamp(requested, minimum, maximum);
    body.style.setProperty("--right-panel-width", `${width}px`);
    workspaceResizer.setAttribute("aria-valuemin", String(Math.round(minimum)));
    workspaceResizer.setAttribute("aria-valuemax", String(Math.round(maximum)));
    workspaceResizer.setAttribute("aria-valuenow", String(Math.round(width)));
    localStorage.setItem("lyrnova.rightPanelWidth", String(width));
  };

  const startDrag = (handle, update) => (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    handle.classList.add("active");
    document.body.classList.add("resizing-panels");
    handle.setPointerCapture(event.pointerId);

    const move = (moveEvent) => update(moveEvent.clientX);
    const stop = () => {
      handle.classList.remove("active");
      document.body.classList.remove("resizing-panels");
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  };

  sidebarResizer.addEventListener("pointerdown", startDrag(sidebarResizer, (clientX) => {
    const activityWidth = document.querySelector(".activity-bar").getBoundingClientRect().width;
    setSidebarWidth(clientX - appShell.getBoundingClientRect().left - activityWidth);
  }));
  workspaceResizer.addEventListener("pointerdown", startDrag(workspaceResizer, (clientX) => {
    const bounds = document.querySelector(".workspace-body").getBoundingClientRect();
    setRightPanelWidth(bounds.right - clientX);
  }));

  sidebarResizer.addEventListener("dblclick", () => setSidebarWidth(250));
  workspaceResizer.addEventListener("dblclick", () => setRightPanelWidth(390));
  sidebarResizer.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const current = Number(sidebarResizer.getAttribute("aria-valuenow"));
    setSidebarWidth(current + (event.key === "ArrowRight" ? 12 : -12));
  });
  workspaceResizer.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const current = Number(workspaceResizer.getAttribute("aria-valuenow"));
    setRightPanelWidth(current + (event.key === "ArrowLeft" ? 12 : -12));
  });

  constrainPanelWidths = () => {
    setSidebarWidth(Number(sidebarResizer.getAttribute("aria-valuenow")) || 250);
    setRightPanelWidth(Number(workspaceResizer.getAttribute("aria-valuenow")) || 390);
  };
  setSidebarWidth(restore("lyrnova.sidebarWidth", 250));
  setRightPanelWidth(restore("lyrnova.rightPanelWidth", 390));
  window.addEventListener("resize", constrainPanelWidths);
}

function openPalette() {
  previousFocus = document.activeElement;
  palette.hidden = false;
  paletteInput.value = "";
  paletteInput.focus();
}

function closePalette() {
  if (palette.hidden) return;
  palette.hidden = true;
  previousFocus?.focus();
}

function openAccount() {
  if (!aiProviderEnabled) {
    announce("Nenhum provider de IA está instalado e ativo");
    return;
  }
  closePalette();
  integrationNote.hidden = true;
  accountDialog.showModal();
}

function closeAccount() {
  accountDialog.close();
}

function formatPlan(planType) {
  if (!planType) return null;
  return planType
    .replaceAll("_", " ")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function accountModeLabel(authMode) {
  if (authMode === "chat_gpt") return "Conta ChatGPT";
  if (authMode === "api_key") return "Chave de API OpenAI";
  return "Conta do agente";
}

function renderAccount(status) {
  const account = status?.account;
  if (!account) {
    profileAvatar.textContent = "↗";
    profileAvatar.classList.add("signed-out");
    profileAccountName.textContent = "Conectar conta";
    profileAccountPlan.textContent = activeAiProvider?.name || "Provider de IA";
    accountTitle.textContent = `Entrar com ${activeAiProvider?.name || "o provider de IA"}`;
    accountCopy.textContent = "Use uma forma de autenticação oferecida pelo provider ativo para acessar o agente.";
    accountPrivacyTitle.textContent = "Sua senha nunca passa pelo Lyrnova";
    accountPrivacyCopy.textContent = "Tokens e cookies ficam fora do frontend.";
    loginOptions.hidden = false;
    logoutButton.hidden = true;
    return;
  }

  const mode = accountModeLabel(account.authMode);
  const plan = formatPlan(account.planType);
  const identity = account.email || mode;
  profileAvatar.textContent = account.email?.trim().charAt(0).toUpperCase() || "✓";
  profileAvatar.classList.remove("signed-out");
  profileAccountName.textContent = identity;
  profileAccountPlan.textContent = plan ? `Plano ${plan}` : mode;
  accountTitle.textContent = "Conta conectada";
  accountCopy.textContent = account.email
    ? `${account.email}${plan ? ` · Plano ${plan}` : ""}`
    : `${mode}${plan ? ` · Plano ${plan}` : ""}`;
  accountPrivacyTitle.textContent = `Sessão gerenciada por ${activeAiProvider?.name || "provider de IA"}`;
  accountPrivacyCopy.textContent = "O frontend recebe somente e-mail, tipo de conta e plano; credenciais não são expostas.";
  loginOptions.hidden = true;
  logoutButton.hidden = false;
}

function renderAccountUnavailable() {
  profileAvatar.textContent = "!";
  profileAvatar.classList.add("signed-out");
  profileAccountName.textContent = "Provider indisponível";
  profileAccountPlan.textContent = "Verifique a instalação local";
  accountTitle.textContent = "Provider de IA indisponível";
  accountCopy.textContent = "Não foi possível consultar a sessão do provider ativo neste momento.";
  accountPrivacyTitle.textContent = "Nenhuma credencial foi acessada";
  accountPrivacyCopy.textContent = "A consulta falhou antes de disponibilizar dados de conta ao frontend.";
  loginOptions.hidden = true;
  logoutButton.hidden = true;
}

async function loadAgentAccount() {
  if (!aiProviderEnabled) return;
  if (!invoke) {
    renderAccount(null);
    return;
  }
  try {
    renderAccount(await invoke("agent_account_read"));
  } catch {
    renderAccountUnavailable();
  }
}

async function logoutAgentAccount() {
  if (!invoke || !aiProviderEnabled) return;
  logoutButton.disabled = true;
  try {
    renderAccount(await invoke("agent_logout"));
    integrationNote.textContent = "Sessão encerrada.";
    integrationNote.hidden = false;
  } catch {
    integrationNote.textContent = "Não foi possível encerrar a sessão.";
    integrationNote.hidden = false;
  } finally {
    logoutButton.disabled = false;
  }
}

function hasDirtyDocuments() {
  return [...draftDocuments.keys()].some((path) => isDirty(path));
}

function setMaximizeControl(maximized) {
  const button = document.querySelector("#window-maximize");
  button.textContent = maximized ? "❐" : "□";
  button.title = maximized ? "Restaurar" : "Maximizar";
  button.setAttribute("aria-label", button.title);
}

async function syncWindowState() {
  if (!currentWindow) return;
  try { setMaximizeControl(await currentWindow.isMaximized()); } catch { /* estado visual não crítico */ }
}

function bindWindowControls() {
  const topbar = document.querySelector(".context-bar");
  const minimize = document.querySelector("#window-minimize");
  const maximize = document.querySelector("#window-maximize");
  const close = document.querySelector("#window-close");

  topbar.addEventListener("mousedown", (event) => {
    if (event.button !== 0 || !currentWindow) return;
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (target?.closest("button, input, textarea, a, [role='button'], .window-controls")) return;
    void currentWindow.startDragging();
  });
  minimize.addEventListener("click", () => { void currentWindow?.minimize(); });
  maximize.addEventListener("click", async () => {
    if (!currentWindow) return;
    try {
      await currentWindow.toggleMaximize();
      await syncWindowState();
    } catch { /* mantém o shell utilizável */ }
  });
  close.addEventListener("click", () => {
    if (hasDirtyDocuments() && !window.confirm("Há rascunhos alterados em memória. Fechar o Lyrnova e descartá-los?")) return;
    void currentWindow?.close();
  });
  setMaximizeControl(false);
  void syncWindowState();
}

function documentName(path) {
  return path.split("/").at(-1);
}

function languageIdFor(path) {
  const name = documentName(path).toLocaleLowerCase("en-US");
  const extension = name.includes(".") ? name.split(".").at(-1) : "";
  if (["dockerfile", "containerfile"].includes(name)) return "dockerfile";
  return ({
    c: "c", cc: "cpp", cpp: "cpp", css: "css", go: "go", h: "c", hpp: "cpp",
    html: "html", java: "java", js: "javascript", json: "json", jsx: "javascript",
    md: "markdown", py: "python", rs: "rust", sh: "shell", sql: "sql", toml: "ini",
    ts: "typescript", tsx: "typescript", xml: "xml", yaml: "yaml", yml: "yaml",
  })[extension] ?? "plaintext";
}

function languageFor(path) {
  const id = languageIdFor(path);
  return ({ cpp: "C++", css: "CSS", dockerfile: "Dockerfile", go: "Go", html: "HTML", ini: "TOML", javascript: "JavaScript", json: "JSON", markdown: "Markdown", python: "Python", rust: "Rust", shell: "Shell", sql: "SQL", typescript: "TypeScript", xml: "XML", yaml: "YAML" })[id] ?? (id === "plaintext" ? "Texto" : id.toUpperCase());
}

function fileIconFor(path) {
  const extension = path.split(".").at(-1).toLocaleLowerCase("en-US");
  return ({
    rs: ["Rs", "rust"], js: ["JS", "javascript"], jsx: ["JS", "javascript"],
    ts: ["TS", "typescript"], tsx: ["TS", "typescript"], html: ["<>", "html"],
    css: ["#", "css"], json: ["{}", "json"], md: ["M", "markdown"],
    toml: ["T", "toml"], yaml: ["Y", "generic"], yml: ["Y", "generic"],
    png: ["◉", "image"], jpg: ["◉", "image"], jpeg: ["◉", "image"],
    svg: ["◇", "image"], ico: ["◉", "image"], py: ["Py", "generic"],
    go: ["Go", "generic"], sh: [">_", "generic"], sql: ["DB", "generic"],
  })[extension] ?? ["·", "generic"];
}

function documentKind(path) {
  return fileIconFor(path)[0];
}

function gitKindPresentation(kind) {
  return ({
    added: ["A", "Adicionado"],
    modified: ["M", "Modificado"],
    deleted: ["D", "Removido"],
    renamed: ["R", "Renomeado"],
    copied: ["C", "Copiado"],
    type_changed: ["T", "Tipo alterado"],
    untracked: ["U", "Não rastreado"],
    conflicted: ["!", "Conflito"],
  })[kind] ?? ["?", "Estado desconhecido"];
}

function renderGitList(container, entries, side) {
  container.replaceChildren();
  if (!entries.length) {
    const empty = document.createElement("p");
    empty.className = "git-empty";
    empty.textContent = side === "index" ? "Nenhuma alteração preparada" : "Workspace limpo";
    container.append(empty);
    return;
  }

  entries.forEach((change) => {
    const kind = change[side];
    const [letter, title] = gitKindPresentation(kind);
    const row = document.createElement("div");
    row.className = "git-file-row";
    const open = document.createElement("button");
    open.type = "button";
    open.className = "git-file-open";
    if (kind !== "deleted") open.dataset.file = change.path;
    open.title = change.previousPath
      ? `${title}: ${change.previousPath} → ${change.path}`
      : `${title}: ${change.path}`;

    const status = document.createElement("span");
    status.className = `git-state ${kind}`;
    status.textContent = letter;
    const [icon, iconClass] = fileIconFor(change.path);
    const fileIcon = document.createElement("span");
    fileIcon.className = `file-type-icon ${iconClass}`;
    fileIcon.textContent = icon;
    const pathParts = change.path.split("/");
    const name = pathParts.pop();
    const label = document.createElement("span");
    label.append(document.createTextNode(name));
    const directory = document.createElement("small");
    directory.textContent = pathParts.join("/") || ".";
    label.append(directory);
    open.append(status, fileIcon, label);
    const action = document.createElement("button");
    action.type = "button";
    action.className = "git-file-action";
    action.dataset.gitAction = side === "index" ? "unstage" : "stage";
    action.dataset.gitPath = change.path;
    action.textContent = side === "index" ? "−" : "+";
    action.title = side === "index" ? `Remover ${change.path} da preparação` : `Preparar ${change.path}`;
    action.setAttribute("aria-label", action.title);
    action.disabled = gitMutationRunning;
    row.append(open, action);
    container.append(row);
  });
}

function updateGitCommitButton() {
  const hasStaged = currentGitStatus?.changes?.some((change) => change.index) ?? false;
  gitCommitButton.disabled = gitMutationRunning || !hasStaged || !gitCommitMessage.value.trim();
}

function renderGitStatus(status) {
  currentGitStatus = status;
  const staged = status.changes.filter((change) => change.index);
  const worktree = status.changes.filter((change) => change.worktree);
  const total = status.changes.length;
  const countLabel = `${total} ${total === 1 ? "alteração" : "alterações"}`;
  const tracking = [status.ahead ? `↑${status.ahead}` : "", status.behind ? `↓${status.behind}` : ""].filter(Boolean).join(" ");

  gitBranchName.textContent = status.branch;
  contextBranch.textContent = status.branch;
  gitBranchSummary.textContent = tracking ? `${countLabel} · ${tracking}` : countLabel;
  projectGitSummary.textContent = `${status.branch} · ${countLabel}`;
  gitActivityBadge.textContent = String(total);
  gitActivityBadge.hidden = total === 0;
  gitStagedCount.textContent = String(staged.length);
  gitChangesCount.textContent = String(worktree.length);
  gitStagedGroup.hidden = staged.length === 0;
  renderGitList(gitStagedList, staged, "index");
  renderGitList(gitChangesList, worktree, "worktree");
  gitNote.textContent = status.upstream
    ? `Acompanha ${status.upstream}${status.commit ? ` · ${status.commit}` : ""} · push desativado`
    : `${status.commit ? `${status.commit} · ` : ""}push desativado`;
  updateGitCommitButton();
}

async function loadGitStatus() {
  if (!invoke) return;
  gitBranchSummary.textContent = "Atualizando…";
  try {
    renderGitStatus(await invoke("git_status"));
  } catch (error) {
    currentGitStatus = null;
    updateGitCommitButton();
    const unavailable = error?.code === "git_unavailable";
    gitBranchName.textContent = "Git indisponível";
    gitBranchSummary.textContent = unavailable ? "Executável git não encontrado" : "Não foi possível ler o repositório";
    projectGitSummary.textContent = "Git indisponível";
    gitChangesList.replaceChildren();
    const message = document.createElement("p");
    message.className = "git-empty";
    message.textContent = "O status não pôde ser carregado. Seus arquivos não foram alterados.";
    gitChangesList.append(message);
  }
}

async function runGitFileAction(action, path) {
  if (!invoke || gitMutationRunning || !["stage", "unstage"].includes(action)) return;
  gitMutationRunning = true;
  updateGitCommitButton();
  try {
    const status = await invoke(action === "stage" ? "git_stage" : "git_unstage", { path });
    renderGitStatus(status);
    announce(action === "stage" ? `${path} preparado` : `${path} removido da preparação`);
  } catch {
    announce("O Git recusou a alteração. Atualize o status e tente novamente");
    await loadGitStatus();
  } finally {
    gitMutationRunning = false;
    updateGitCommitButton();
  }
}

async function commitGitChanges() {
  if (!invoke || gitMutationRunning || gitCommitButton.disabled) return;
  gitMutationRunning = true;
  updateGitCommitButton();
  try {
    const status = await invoke("git_commit", { message: gitCommitMessage.value });
    gitCommitMessage.value = "";
    renderGitStatus(status);
    announce("Commit criado localmente");
  } catch (error) {
    announce(error?.code === "invalid_message" ? "Informe uma mensagem de commit válida" : "Não foi possível criar o commit local");
  } finally {
    gitMutationRunning = false;
    updateGitCommitButton();
  }
}

function isDirty(path) {
  return draftDocuments.get(path) !== savedDocuments.get(path);
}

function renderEditorTabs() {
  editorTabs.replaceChildren();
  openDocuments.forEach((path) => {
    const tab = document.createElement("div");
    tab.className = `editor-tab${path === activeDocument ? " active" : ""}`;

    const select = document.createElement("button");
    select.className = "editor-tab-select";
    select.type = "button";
    select.role = "tab";
    select.dataset.editorPath = path;
    select.setAttribute("aria-selected", String(path === activeDocument));

    const kind = document.createElement("span");
    const [icon, iconClass] = fileIconFor(path);
    kind.className = `tab-kind ${iconClass}`;
    kind.textContent = icon;
    const name = document.createElement("span");
    name.className = "tab-name";
    name.textContent = documentName(path);
    const dirty = document.createElement("span");
    dirty.className = "dirty-mark";
    dirty.textContent = isDirty(path) ? "●" : "";
    select.append(kind, name, dirty);

    const close = document.createElement("button");
    close.className = "editor-tab-close";
    close.type = "button";
    close.dataset.closeEditorPath = path;
    close.title = `Fechar ${documentName(path)}`;
    close.setAttribute("aria-label", close.title);
    close.textContent = "×";
    tab.append(select, close);
    editorTabs.append(tab);
  });
}

async function closeDocument(path) {
  if (ideSettings.confirmDirtyClose && isDirty(path) && !window.confirm(`Fechar ${documentName(path)} e descartar as alterações não salvas?`)) return;
  const index = openDocuments.indexOf(path);
  if (index < 0) return;
  openDocuments.splice(index, 1);

  if (isDirty(path)) draftDocuments.set(path, savedDocuments.get(path));
  editorModels.get(path)?.dispose();
  editorModels.delete(path);

  if (activeDocument !== path) {
    renderEditorTabs();
    return;
  }

  const next = openDocuments[Math.min(index, openDocuments.length - 1)];
  if (next) {
    await openDocument(next);
    return;
  }

  activeDocument = null;
  codeEditor.setModel(null);
  editorBreadcrumb.textContent = "Nenhum arquivo aberto";
  languageMode.textContent = "Texto";
  draftState.lastChild.textContent = " Abra um arquivo no Explorer";
  editorWorkspace.dataset.saveState = "clean";
  renderEditorTabs();
}

function renderBreadcrumb(path) {
  editorBreadcrumb.replaceChildren();
  path.split("/").forEach((part, index, parts) => {
    const label = document.createElement("span");
    label.textContent = part;
    editorBreadcrumb.append(label);
    if (index < parts.length - 1) editorBreadcrumb.append(document.createTextNode("›"));
  });
}

function updateCursorPosition() {
  const position = codeEditor?.getPosition();
  if (position) cursorPosition.textContent = `Ln ${position.lineNumber}, Col ${position.column}`;
}

function updateEditorEmptyState() {
  const empty = !activeDocument || !codeEditor?.getModel();
  editorEmptyState.hidden = !empty;
  editorWorkspace.dataset.empty = String(empty);
}

function updateDraftState() {
  const dirty = isDirty(activeDocument);
  editorWorkspace.dataset.clean = String(!dirty);
  editorWorkspace.dataset.saveState = dirty ? "dirty" : "clean";
  draftState.lastChild.textContent = dirty ? " Alteração não salva" : (documentRevisions.has(activeDocument) ? " Salvo no disco" : " Rascunho em memória");
  renderEditorTabs();
}

function approvalKindLabel(kind) {
  return ({
    command: "Comando",
    file_change: "Alteração de arquivos",
    network: "Rede",
    write_stdin: "Entrada de processo",
  })[kind] ?? "Ação";
}

function renderApprovalState(state) {
  currentApprovalState = state;
  approvalSessionRules.replaceChildren();
  if (!state.sessionRules?.length) {
    const empty = document.createElement("p");
    empty.className = "plugin-list-empty";
    empty.textContent = "Nenhuma regra ativa.";
    approvalSessionRules.append(empty);
  } else {
    state.sessionRules.forEach((rule) => {
      const row = document.createElement("article");
      row.className = "approval-state-row";
      const copy = document.createElement("div");
      const title = document.createElement("strong");
      title.textContent = approvalKindLabel(rule.kind);
      const scope = document.createElement("p");
      scope.textContent = rule.scope;
      const hash = document.createElement("code");
      hash.textContent = `SHA-256 ${rule.actionSha256}`;
      copy.append(title, scope, hash);
      const revoke = document.createElement("button");
      revoke.type = "button";
      revoke.className = "danger-button subtle-danger";
      revoke.dataset.approvalRuleRevoke = rule.ruleId;
      revoke.textContent = "Revogar";
      revoke.setAttribute("aria-label", `Revogar regra de ${title.textContent}`);
      row.append(copy, revoke);
      approvalSessionRules.append(row);
    });
  }

  approvalAuditList.replaceChildren();
  if (!state.audit?.length && !recentProcessAudits.length) {
    const empty = document.createElement("p");
    empty.className = "plugin-list-empty";
    empty.textContent = "Nenhuma decisão nesta sessão.";
    approvalAuditList.append(empty);
    return;
  }
  const decisions = {
    accept: "Permitida uma vez",
    accept_for_session: "Permitida na sessão",
    decline: "Negada",
    cancel: "Turno cancelado",
  };
  const sources = { user: "usuário", session_rule: "regra da sessão", timeout: "expiração", lifecycle: "fim da sessão" };
  state.audit.forEach((entry) => {
    const row = document.createElement("article");
    row.className = "approval-state-row approval-audit-row";
    const copy = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = `${approvalKindLabel(entry.kind)} · ${decisions[entry.decision] ?? entry.decision}`;
    const detail = document.createElement("p");
    const time = new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" }).format(new Date(entry.decidedAtMs));
    detail.textContent = `${sources[entry.source] ?? entry.source} · ${time}`;
    const hash = document.createElement("code");
    hash.textContent = `SHA-256 ${entry.actionSha256}`;
    copy.append(title, detail, hash);
    row.append(copy);
    approvalAuditList.append(row);
  });
  const processPhases = { reviewed: "Revisada", started: "Iniciada", completed: "Concluída" };
  recentProcessAudits.forEach((entry) => {
    const row = document.createElement("article");
    row.className = "approval-state-row approval-audit-row";
    const copy = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = `Task/processo · ${processPhases[entry.phase] ?? entry.phase}`;
    const detail = document.createElement("p");
    const time = new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" }).format(new Date(entry.observedAtMs));
    detail.textContent = `${entry.origin} · ${time}`;
    const hash = document.createElement("code");
    hash.textContent = `SHA-256 do comando ${entry.commandSha256}`;
    copy.append(title, detail, hash);
    row.append(copy);
    approvalAuditList.append(row);
  });
}

async function loadApprovalState() {
  if (!invoke || !approvalSessionRules || !approvalAuditList) return;
  try {
    renderApprovalState(await invoke("agent_approval_state"));
  } catch {
    approvalSessionRules.textContent = "Não foi possível consultar as aprovações desta sessão.";
    approvalAuditList.replaceChildren();
  }
}

async function revokeApprovalRule(ruleId, button) {
  if (!invoke || !ruleId) return;
  button.disabled = true;
  try {
    await invoke("agent_approval_session_revoke", { ruleId });
    await loadApprovalState();
    announce("Regra de aprovação revogada");
  } catch {
    button.disabled = false;
    announce("A regra já não estava ativa");
  }
}

function setEditorError(message) {
  editorWorkspace.dataset.saveState = "error";
  draftState.lastChild.textContent = ` ${message}`;
  announce(message);
}

function showWorkspaceView(view, focusEditor = false) {
  if (view === "settings") {
    toggleInspector(false);
    appShell.dataset.workspaceView = "settings";
    appShell.dataset.chatOpen = "false";
    conversation.classList.remove("visible");
    editorWorkspace.hidden = true;
    settingsWorkspace.hidden = false;
    void loadApprovalState();
    if (narrowWorkspace.matches) appShell.dataset.sidebarOpen = "false";
    return;
  }
  appShell.dataset.workspaceView = "editor";
  settingsWorkspace.hidden = true;
  editorWorkspace.hidden = false;
  if (view === "agent") {
    if (!aiProviderEnabled) {
      announce("Instale e ative um plugin de IA para abrir conversas");
      return;
    }
    toggleInspector(false);
    appShell.dataset.chatOpen = "true";
    conversation.classList.add("visible");
    if (narrowWorkspace.matches) appShell.dataset.sidebarOpen = "false";
    window.requestAnimationFrame(() => {
      constrainPanelWidths();
      codeEditor?.layout();
    });
    announce("Chat do agente focado à direita");
    return;
  }
  appShell.dataset.chatOpen = "false";
  conversation.classList.remove("visible");
  if (focusEditor && narrowWorkspace.matches) appShell.dataset.sidebarOpen = "false";
  if (focusEditor) codeEditor?.focus();
  announce("Editor central focado");
}

async function openDocument(path, focusEditor = true, targetLine = null) {
  if (invoke && !documentRevisions.has(path)) {
    try {
      const snapshot = await invoke("workspace_read", { path });
      savedDocuments.set(path, snapshot.content);
      draftDocuments.set(path, snapshot.content);
      documentRevisions.set(path, snapshot.revision);
    } catch (error) {
      if (!draftDocuments.has(path)) {
        setEditorError(["binary_file", "not_utf8"].includes(error?.code) ? "Arquivo binário não pode ser editado" : "Não foi possível abrir o arquivo");
        return;
      }
    }
  }
  if (!draftDocuments.has(path)) return;
  if (!openDocuments.includes(path)) openDocuments.push(path);
  activeDocument = path;
  let model = editorModels.get(path);
  if (!model) {
    const uri = monaco.Uri.from({ scheme: "file", path: `/workspace/${path}` });
    model = monaco.editor.createModel(draftDocuments.get(path), languageIdFor(path), uri);
    editorModels.set(path, model);
  }
  codeEditor.setModel(model);
  if (Number.isInteger(targetLine) && targetLine > 0) {
    const lineNumber = Math.min(targetLine, model.getLineCount());
    codeEditor.setPosition({ lineNumber, column: 1 });
    codeEditor.revealLineInCenter(lineNumber);
  }
  languageMode.textContent = languageFor(path);
  renderBreadcrumb(path);
  updateCursorPosition();
  updateDraftState();
  toggleInspector(false);
  showWorkspaceView("editor", focusEditor);
}

async function saveDocument() {
  const path = activeDocument;
  if (!path || !codeEditor.getModel()) return;
  const submittedContent = codeEditor.getValue();
  draftDocuments.set(path, submittedContent);
  const expectedRevision = documentRevisions.get(path);
  if (!invoke || !expectedRevision) {
    savedDocuments.set(path, submittedContent);
    updateDraftState();
    announce(`${documentName(path)} salvo apenas na memória`);
    return;
  }

  try {
    const snapshot = await invoke("workspace_save", {
      request: {
        path,
        content: submittedContent,
        expectedRevision,
      },
    });
    savedDocuments.set(path, snapshot.content);
    documentRevisions.set(path, snapshot.revision);
    if (activeDocument === path) updateDraftState();
    else renderEditorTabs();
    void loadGitStatus();
    announce(`${documentName(path)} salvo no disco`);
  } catch (error) {
    if (error?.code === "conflict") {
      setEditorError("Conflito: o arquivo mudou no disco; seu rascunho foi preservado");
    } else {
      setEditorError("Falha ao salvar; seu rascunho foi preservado");
    }
  }
}

function renderWorkspaceTree() {
  fileTree.replaceChildren();
  if (!workspaceEntries.length) {
    const empty = document.createElement("p");
    empty.className = "git-empty";
    empty.textContent = currentProject ? "Este projeto não contém arquivos visíveis." : "Abra ou crie um projeto para explorar arquivos.";
    fileTree.append(empty);
    return;
  }
  workspaceEntries.forEach((entry) => {
    const parts = entry.path.split("/");
    const hiddenByParent = parts.slice(0, -1).some((_, index) => collapsedDirectories.has(parts.slice(0, index + 1).join("/")));
    if (hiddenByParent) return;

    const button = document.createElement("button");
    button.type = "button";
    button.role = "treeitem";
    button.style.setProperty("--tree-depth", String(entry.path.split("/").length - 1));
    button.classList.toggle("selected", entry.path === selectedWorkspacePath);
    if (entry.kind === "directory") {
      const expanded = !collapsedDirectories.has(entry.path);
      button.setAttribute("aria-expanded", String(expanded));
      button.classList.add("workspace-directory");
      button.dataset.directory = entry.path;
      const chevron = document.createElement("span");
      chevron.className = "folder-chevron";
      chevron.textContent = expanded ? "▾" : "▸";
      const icon = document.createElement("span");
      icon.className = "folder-icon";
      icon.setAttribute("aria-hidden", "true");
      const name = document.createElement("span");
      name.textContent = entry.name;
      button.append(chevron, icon, name);
    } else {
      button.classList.add("workspace-file");
      button.dataset.file = entry.path;
      const kind = document.createElement("span");
      const [icon, iconClass] = fileIconFor(entry.path);
      kind.className = `file-type-icon ${iconClass}`;
      kind.textContent = icon;
      const name = document.createElement("span");
      name.textContent = entry.name;
      button.title = entry.path;
      button.append(kind, name);
    }
    fileTree.append(button);
  });
}

function renderWorkspaceSearchResults(results, query) {
  fileTree.replaceChildren();
  if (!results.length) {
    const empty = document.createElement("p");
    empty.className = "git-empty";
    empty.textContent = `Nenhum resultado para “${query}”.`;
    fileTree.append(empty);
    return;
  }
  results.forEach((result) => {
    const button = document.createElement("button");
    button.type = "button";
    button.role = "treeitem";
    button.className = "workspace-search-result";
    button.classList.toggle("selected", result.path === selectedWorkspacePath);
    if (result.entryKind === "directory") button.dataset.directory = result.path;
    else button.dataset.file = result.path;
    if (result.line) button.dataset.line = String(result.line);
    const kind = document.createElement("span");
    if (result.entryKind === "directory") {
      kind.className = "folder-icon";
      kind.setAttribute("aria-hidden", "true");
    } else {
      const [icon, iconClass] = fileIconFor(result.path);
      kind.className = `file-type-icon ${iconClass}`;
      kind.textContent = icon;
    }
    const copy = document.createElement("span");
    copy.className = "workspace-search-copy";
    const title = document.createElement("strong");
    title.textContent = result.line ? `${result.path}:${result.line}:${result.column ?? 1}` : result.path;
    const preview = document.createElement("small");
    preview.textContent = result.preview;
    copy.append(title, preview);
    button.append(kind, copy);
    fileTree.append(button);
  });
}

async function searchWorkspace() {
  const query = fileFilter.value.trim();
  const sequence = ++workspaceSearchSequence;
  if (!query) {
    renderWorkspaceTree();
    return;
  }
  if (!invoke) return;
  try {
    const results = await invoke("workspace_search", { query });
    if (sequence === workspaceSearchSequence && query === fileFilter.value.trim()) {
      renderWorkspaceSearchResults(results, query);
    }
  } catch {
    if (sequence === workspaceSearchSequence) {
      renderWorkspaceSearchResults([], query);
      announce("A busca do workspace foi recusada");
    }
  }
}

function scheduleWorkspaceSearch() {
  window.clearTimeout(workspaceSearchTimer);
  workspaceSearchTimer = window.setTimeout(() => void searchWorkspace(), 180);
}

async function loadWorkspaceTree(resetCollapse = true) {
  if (!invoke) return;
  try {
    workspaceEntries = await invoke("workspace_list");
    if (resetCollapse) {
      collapsedDirectories.clear();
      workspaceEntries
        .filter((entry) => entry.kind === "directory")
        .forEach((entry) => collapsedDirectories.add(entry.path));
    } else {
      const directories = new Set(workspaceEntries.filter((entry) => entry.kind === "directory").map((entry) => entry.path));
      [...collapsedDirectories].forEach((path) => { if (!directories.has(path)) collapsedDirectories.delete(path); });
    }
    if (selectedWorkspacePath && !workspaceEntries.some((entry) => entry.path === selectedWorkspacePath)) selectedWorkspacePath = null;
    if (fileFilter.value.trim()) await searchWorkspace();
    else renderWorkspaceTree();
  } catch {
    workspaceEntries = [];
    renderWorkspaceTree();
  }
}

function selectedEntry() {
  return workspaceEntries.find((entry) => entry.path === selectedWorkspacePath) ?? null;
}

function workspaceParentForCreation() {
  const entry = selectedEntry();
  if (!entry) return "";
  if (entry.kind === "directory") return `${entry.path}/`;
  const separator = entry.path.lastIndexOf("/");
  return separator >= 0 ? entry.path.slice(0, separator + 1) : "";
}

function workspaceErrorMessage(error) {
  const messages = {
    entry_already_exists: "Já existe uma entrada nesse caminho.",
    invalid_path: "Use um caminho relativo válido dentro do projeto.",
    path_escapes_workspace: "O caminho sairia do projeto.",
    symbolic_link: "Links simbólicos não podem ser usados nesta operação.",
    conflict: "O arquivo mudou no disco; atualize e tente novamente.",
    recovery_unavailable: "A área segura de recuperação não está disponível.",
    unknown_recovery: "Esta exclusão não pode mais ser desfeita.",
  };
  return messages[error?.code] ?? "A operação de arquivo foi recusada.";
}

async function createWorkspaceDocument() {
  if (!invoke || !currentProject) return;
  const path = window.prompt("Caminho do novo arquivo, relativo ao projeto:", workspaceParentForCreation());
  if (path === null) return;
  try {
    const snapshot = await invoke("workspace_create_document", { request: { path, content: "" } });
    selectedWorkspacePath = snapshot.path;
    await loadWorkspaceTree(false);
    await openDocument(snapshot.path);
    void loadGitStatus();
    announce(`${snapshot.path} criado`);
  } catch (error) {
    announce(workspaceErrorMessage(error));
  }
}

async function createWorkspaceDirectory() {
  if (!invoke || !currentProject) return;
  const path = window.prompt("Caminho da nova pasta, relativo ao projeto:", workspaceParentForCreation());
  if (path === null) return;
  try {
    const entry = await invoke("workspace_create_directory", { path });
    selectedWorkspacePath = entry.path;
    collapsedDirectories.delete(entry.path);
    await loadWorkspaceTree(false);
    void loadGitStatus();
    announce(`${entry.path} criada`);
  } catch (error) {
    announce(workspaceErrorMessage(error));
  }
}

function affectedDocuments(path) {
  return [...new Set([...openDocuments, ...draftDocuments.keys()])]
    .filter((candidate) => candidate === path || candidate.startsWith(`${path}/`));
}

async function purgeDocumentStates(paths) {
  for (const path of paths) {
    if (openDocuments.includes(path)) await closeDocument(path);
    editorModels.get(path)?.dispose();
    editorModels.delete(path);
    savedDocuments.delete(path);
    draftDocuments.delete(path);
    documentRevisions.delete(path);
  }
}

async function moveWorkspaceEntry() {
  const entry = selectedEntry();
  if (!invoke || !entry) { announce("Selecione um arquivo ou pasta para mover"); return; }
  const affected = affectedDocuments(entry.path);
  if (affected.some(isDirty)) { announce("Salve ou descarte os rascunhos afetados antes de mover"); return; }
  const destination = window.prompt("Novo caminho relativo ao projeto:", entry.path);
  if (destination === null || destination === entry.path) return;
  try {
    const moved = await invoke("workspace_move", { request: { source: entry.path, destination } });
    await purgeDocumentStates(affected);
    selectedWorkspacePath = moved.path;
    await loadWorkspaceTree(false);
    if (moved.kind === "file") await openDocument(moved.path);
    void loadGitStatus();
    announce(`${entry.path} movido para ${moved.path}`);
  } catch (error) {
    announce(workspaceErrorMessage(error));
  }
}

async function deleteWorkspaceEntry() {
  const entry = selectedEntry();
  if (!invoke || !entry) { announce("Selecione um arquivo ou pasta para excluir"); return; }
  const affected = affectedDocuments(entry.path);
  if (affected.some(isDirty)) { announce("Salve ou descarte os rascunhos afetados antes de excluir"); return; }
  if (!window.confirm(`Excluir “${entry.path}”? A exclusão poderá ser desfeita durante esta sessão.`)) return;
  try {
    const expectedRevision = entry.kind === "file" ? (documentRevisions.get(entry.path) ?? null) : null;
    const recovery = await invoke("workspace_delete", { request: { path: entry.path, expectedRevision } });
    workspaceRecoveries.push(recovery);
    await purgeDocumentStates(affected);
    selectedWorkspacePath = null;
    workspaceRecoveryCopy.textContent = `${entry.path} removido`;
    workspaceRecovery.hidden = false;
    await loadWorkspaceTree(false);
    void loadGitStatus();
    announce(`${entry.path} excluído; use Desfazer para restaurar`);
  } catch (error) {
    announce(workspaceErrorMessage(error));
  }
}

async function restoreWorkspaceEntry() {
  const latestRecovery = workspaceRecoveries.at(-1);
  if (!invoke || !latestRecovery) return;
  try {
    const entry = await invoke("workspace_restore", { recoveryToken: latestRecovery.recoveryToken });
    workspaceRecoveries.pop();
    const previous = workspaceRecoveries.at(-1);
    workspaceRecovery.hidden = !previous;
    if (previous) workspaceRecoveryCopy.textContent = `${previous.path} removido`;
    selectedWorkspacePath = entry.path;
    await loadWorkspaceTree(false);
    void loadGitStatus();
    announce(`${entry.path} restaurado`);
  } catch (error) {
    announce(workspaceErrorMessage(error));
  }
}

function renderProjectSummary(project) {
  currentProject = project;
  projectName.textContent = project.name;
  contextProjectName.textContent = project.name;
  contextProjectPath.textContent = project.path;
  projectMark.textContent = project.name.trim().charAt(0).toUpperCase() || "P";
}

async function loadProjectSummary() {
  if (!invoke) {
    renderNoProject();
    return false;
  }
  try {
    renderProjectSummary(await invoke("project_current"));
    return true;
  } catch {
    renderNoProject();
    return false;
  }
}

function renderNoProject() {
  currentProject = null;
  projectName.textContent = "Nenhum projeto";
  contextProjectName.textContent = "Lyrnova";
  contextProjectPath.textContent = "Abra ou crie um projeto";
  projectMark.textContent = "+";
  projectGitSummary.textContent = "Selecionar workspace";
  contextBranch.textContent = "—";
  currentGitStatus = null;
  workspaceEntries = [];
  clearEditorWorkspace();
  renderWorkspaceTree();
  updateGitCommitButton();
}

function clearEditorWorkspace() {
  editorModels.forEach((model) => model.dispose());
  editorModels.clear();
  savedDocuments.clear();
  draftDocuments.clear();
  documentRevisions.clear();
  openDocuments.splice(0, openDocuments.length);
  selectedWorkspacePath = null;
  workspaceRecoveries.splice(0, workspaceRecoveries.length);
  workspaceRecovery.hidden = true;
  fileFilter.value = "";
  activeDocument = null;
  codeEditor.setModel(null);
  editorTabs.replaceChildren();
  editorBreadcrumb.replaceChildren();
  draftState.lastChild.textContent = " Nenhum arquivo aberto";
}

async function openProjectDialog() {
  if (!invoke) return;
  if (agentTurnRunning) { announce("Aguarde o turno atual terminar antes de trocar de projeto"); return; }
  if (runningTask) { announce("Cancele ou aguarde a Task atual antes de trocar de projeto"); return; }
  if (hasDirtyDocuments() && !window.confirm("Há arquivos não salvos. Abrir outro projeto descartará esses rascunhos. Continuar?")) return;
  try {
    const project = await invoke("project_open_dialog");
    if (!project) return;
    renderProjectSummary(project);
    clearEditorWorkspace();
    activeAgentThreadId = null;
    streamedAgentMessages.clear();
    terminalOutput.textContent = `Terminal local do Lyrnova · ${project.path}\n`;
    await Promise.all([loadWorkspaceTree(), loadGitStatus(), loadTasks()]);
    const firstFile = workspaceEntries.find((entry) => entry.kind === "file");
    if (firstFile) await openDocument(firstFile.path, false);
    if (!terminal.hidden) await startTerminal();
    announce(`Projeto ${project.name} aberto`);
  } catch {
    announce("Não foi possível abrir o projeto selecionado");
  }
}

function openCreateProjectDialog() {
  if (agentTurnRunning) {
    announce("Aguarde o turno atual terminar antes de criar um projeto");
    return;
  }
  if (runningTask) {
    announce("Cancele ou aguarde a Task atual antes de criar um projeto");
    return;
  }
  projectCreateNote.hidden = true;
  newProjectName.value = "";
  newProjectGit.checked = true;
  projectDialog.showModal();
  newProjectName.focus();
}

function closeCreateProjectDialog() {
  if (projectCreationRunning) return;
  projectDialog.close();
}

async function createProject() {
  if (!invoke || projectCreationRunning) return;
  if (agentTurnRunning) {
    announce("Aguarde o turno atual terminar antes de criar um projeto");
    return;
  }
  if (hasDirtyDocuments() && !window.confirm("Há arquivos não salvos. Criar outro projeto descartará esses rascunhos. Continuar?")) return;
  projectCreationRunning = true;
  projectSubmit.disabled = true;
  projectSubmit.textContent = "Criando…";
  projectCreateNote.hidden = true;
  try {
    const project = await invoke("project_create_dialog", {
      name: newProjectName.value,
      initializeGit: newProjectGit.checked,
    });
    if (!project) return;
    closeCreateProjectDialog();
    renderProjectSummary(project);
    clearEditorWorkspace();
    activeAgentThreadId = null;
    streamedAgentMessages.clear();
    terminalOutput.textContent = `Terminal local do Lyrnova · ${project.path}\n`;
    await Promise.all([loadWorkspaceTree(), loadGitStatus(), loadTasks()]);
    const firstFile = workspaceEntries.find((entry) => entry.kind === "file");
    if (firstFile) await openDocument(firstFile.path, false);
    if (!terminal.hidden) await startTerminal();
    announce(project.hasGit ? `Projeto ${project.name} criado com Git` : `Projeto ${project.name} criado`);
  } catch (error) {
    projectCreateNote.textContent = error?.code === "project_already_exists"
      ? "Já existe uma pasta com esse nome no local escolhido."
      : error?.code === "invalid_project_name"
        ? "Use um nome válido, sem caracteres reservados."
        : "Não foi possível criar o projeto.";
    projectCreateNote.hidden = false;
  } finally {
    projectCreationRunning = false;
    projectSubmit.disabled = false;
    projectSubmit.textContent = "Escolher local e criar";
  }
}

function createMessage(role, text) {
  const article = document.createElement("article");
  article.className = `message ${role === "user" ? "user-message" : "agent-message"}`;
  const author = document.createElement("div");
  author.className = "message-author";
  const avatar = document.createElement("span");
  avatar.className = role === "user" ? "avatar small" : "agent-avatar";
  avatar.setAttribute("aria-hidden", "true");
  avatar.textContent = role === "user" ? "RB" : "L";
  const name = document.createElement("strong");
  name.textContent = role === "user" ? "Você" : "Lyrnova";
  const time = document.createElement("time");
  time.textContent = new Intl.DateTimeFormat("pt-BR", { hour: "2-digit", minute: "2-digit" }).format(new Date());
  author.append(avatar, name, time);
  const paragraph = document.createElement("p");
  paragraph.textContent = text;
  article.append(author, paragraph);
  return article;
}

function simulateAgentReply(userText) {
  agentStatus.classList.add("busy");
  agentStatus.lastElementChild.textContent = "Pensando";
  window.setTimeout(() => {
    const reply = createMessage("agent", `Entendi: “${userText}”. Este é o adapter mock do Lyrnova; o próximo passo será transformar essa intenção em eventos tipados antes de permitir qualquer efeito local.`);
    appendMockActivity(reply);
    transcript.append(reply);
    reply.scrollIntoView({ behavior: "smooth", block: "end" });
    agentStatus.classList.remove("busy");
    agentStatus.lastElementChild.textContent = "Pronto";
    announce("Nova resposta do agente");
  }, 520);
}

function setAgentBusy(busy, label = "Pronto") {
  agentTurnRunning = busy;
  agentStatus.classList.toggle("busy", busy);
  agentStatus.lastElementChild.textContent = busy ? "Pensando" : label;
  composer.querySelector(".send-button").disabled = busy;
}

function appendAgentDelta(event) {
  let message = streamedAgentMessages.get(event.itemId);
  if (!message) {
    message = createMessage("agent", "");
    message.dataset.agentItem = event.itemId;
    streamedAgentMessages.set(event.itemId, message);
    transcript.append(message);
  }
  message.querySelector("p").textContent += event.delta;
  message.scrollIntoView({ behavior: "smooth", block: "end" });
}

function createApprovalDetail(label, value) {
  const wrapper = document.createElement("div");
  const term = document.createElement("dt");
  term.textContent = label;
  const description = document.createElement("dd");
  description.textContent = value;
  description.title = value;
  wrapper.append(term, description);
  return wrapper;
}

function createApprovalCard(event) {
  const labels = {
    command: ["Executar comando", "Comando"],
    file_change: ["Alterar arquivos", "Arquivos"],
    network: ["Acessar a rede", "Rede"],
    write_stdin: ["Enviar entrada ao processo", "Processo"],
  };
  const [title, badge] = labels[event.kind] ?? ["Autorizar ação", "Aprovação"];
  const card = document.createElement("article");
  card.className = "approval-card";
  card.setAttribute("role", "region");
  card.dataset.approvalId = event.approvalId;
  card.dataset.threadId = event.threadId;
  card.dataset.turnId = event.turnId;
  card.dataset.itemId = event.itemId;
  card.dataset.actionSha256 = event.actionSha256;

  const heading = document.createElement("div");
  heading.className = "approval-heading";
  const icon = document.createElement("span");
  icon.className = "approval-icon";
  icon.setAttribute("aria-hidden", "true");
  icon.textContent = "!";
  const headingCopy = document.createElement("div");
  const eyebrow = document.createElement("p");
  eyebrow.className = "eyebrow";
  eyebrow.textContent = "O agente solicita permissão";
  const headingTitle = document.createElement("h2");
  headingTitle.id = `approval-title-${event.approvalId}`;
  headingTitle.textContent = title;
  card.setAttribute("aria-labelledby", headingTitle.id);
  headingCopy.append(eyebrow, headingTitle);
  const risk = document.createElement("span");
  risk.className = "risk-badge";
  risk.dataset.risk = event.risk;
  risk.textContent = event.risk === "critical" ? `${badge} · crítica` : `${badge} · atenção`;
  heading.append(icon, headingCopy, risk);
  card.append(heading);

  if (event.kind === "network" && event.networkHost) {
    const network = document.createElement("code");
    network.className = "command-preview";
    network.textContent = `${event.networkProtocol ?? "rede"}://${event.networkHost}`;
    card.append(network);
  } else if (event.command) {
    const command = document.createElement("code");
    command.className = "command-preview";
    command.textContent = event.command;
    card.append(command);
  }

  const details = document.createElement("dl");
  details.className = "approval-details";
  if (event.reason) details.append(createApprovalDetail("Motivo", event.reason));
  if (event.cwd) details.append(createApprovalDetail(event.kind === "file_change" ? "Raiz solicitada" : "Diretório", event.cwd));
  if (event.environmentId) details.append(createApprovalDetail("Dados redigidos", `Valores do ambiente “${event.environmentId}” não são expostos pelo provider`));
  else details.append(createApprovalDetail("Dados redigidos", "Nenhum campo redigido declarado no pedido"));
  if (event.broaderPolicyIgnored) details.append(createApprovalDetail("Ampliação sugerida", "Ignorada; o Lyrnova permite somente este hash exato"));
  if (event.sessionScope) details.append(createApprovalDetail("Escopo da sessão", event.sessionScope));
  details.append(createApprovalDetail("Validade", `${Math.round(event.expiresInMs / 60_000)} min · decisão de uso único`));
  details.append(createApprovalDetail("Hash da ação", event.actionSha256));
  if (event.files?.length) {
    const files = document.createElement("div");
    files.className = "approval-file-list";
    const term = document.createElement("dt");
    term.textContent = `Alterações propostas (${event.files.length})`;
    files.append(term);
    event.files.slice(0, 30).forEach((file) => {
      const item = document.createElement("details");
      const summary = document.createElement("summary");
      summary.textContent = `${file.kind}: ${file.path}`;
      const diff = document.createElement("pre");
      diff.textContent = file.diff || "Diff não fornecido pelo agente.";
      item.append(summary, diff);
      files.append(item);
    });
    if (event.files.length > 30) {
      const remainder = document.createElement("small");
      remainder.textContent = `Mais ${event.files.length - 30} arquivo(s) não exibidos neste cartão.`;
      files.append(remainder);
    }
    details.append(files);
  }
  if (details.childElementCount) card.append(details);

  const actions = document.createElement("div");
  actions.className = "approval-actions";
  const decisions = [
    ["decline", "Negar", "secondary-button"],
    ["cancel", "Cancelar turno", "secondary-button"],
    ["accept_for_session", "Permitir na sessão", "secondary-button"],
    [
      "accept",
      event.risk === "critical" ? "Executar ação crítica" : "Permitir uma vez",
      event.risk === "critical" ? "danger-button" : "accent-button",
    ],
  ];
  decisions.forEach(([decision, label, className]) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = className;
    button.dataset.decision = decision;
    button.textContent = label;
    actions.append(button);
  });
  card.append(actions);
  return card;
}

async function resolveApproval(card, decision) {
  if (!invoke || card.classList.contains("resolving")) return;
  card.classList.add("resolving");
  card.querySelectorAll("button").forEach((button) => { button.disabled = true; });
  try {
    await invoke("agent_approval_resolve", {
      request: {
        approvalId: card.dataset.approvalId,
        threadId: card.dataset.threadId,
        turnId: card.dataset.turnId,
        itemId: card.dataset.itemId,
        actionSha256: card.dataset.actionSha256,
        decision,
      },
    });
  } catch {
    card.classList.remove("resolving");
    card.querySelectorAll("button").forEach((button) => { button.disabled = false; });
    announce("A aprovação expirou ou não pertence mais a este turno");
  }
}

function completeApproval(event) {
  const card = pendingApprovalCards.get(event.approvalId);
  if (!card) return;
  pendingApprovalCards.delete(event.approvalId);
  card.classList.remove("resolving");
  const accepted = ["accept", "accept_for_session"].includes(event.decision);
  card.classList.add(accepted ? "approved" : "declined");
  card.querySelector(".approval-actions")?.remove();
  const result = document.createElement("p");
  result.className = "approval-result";
  result.textContent = event.expired ? "A aprovação expirou e foi negada." : ({
    accept: "Permitido uma vez.",
    accept_for_session: "Permitido para esta sessão do agente.",
    decline: "Ação negada.",
    cancel: "Turno cancelado.",
  })[event.decision] ?? "Aprovação encerrada.";
  card.append(result);
  setAgentBusy(true);
  announce(result.textContent);
  void loadApprovalState();
}

function handleAgentStream(event) {
  if (!event?.type) return;
  if (event.type === "thread_started") activeAgentThreadId = event.threadId;
  if (event.type === "turn_started") setAgentBusy(true);
  if (event.type === "message_delta") appendAgentDelta(event);
  if (event.type === "approval_requested") {
    const card = createApprovalCard(event);
    pendingApprovalCards.set(event.approvalId, card);
    transcript.append(card);
    card.scrollIntoView({ behavior: "smooth", block: "end" });
    setAgentBusy(true);
    agentStatus.lastElementChild.textContent = "Aguardando aprovação";
    announce("O agente está aguardando sua aprovação");
  }
  if (event.type === "approval_resolved") completeApproval(event);
  if (event.type === "approval_session_applied") {
    announce("Uma regra exata desta sessão autorizou a ação automaticamente");
    void loadApprovalState();
  }
  if (event.type === "turn_completed") {
    const completed = event.status === "completed";
    setAgentBusy(false, completed ? "Pronto" : "Falhou");
    if (!completed) {
      const message = event.message || (event.status === "interrupted" ? "Turno interrompido." : "O agente não conseguiu concluir este turno.");
      transcript.append(createMessage("agent", message));
    }
    announce(completed ? "Resposta do agente concluída" : "Turno do agente não concluído");
    void Promise.all([loadWorkspaceTree(), loadGitStatus()]);
  }
}

async function bindAgentStream() {
  if (!listen) return;
  try {
    await listen("agent-stream", ({ payload }) => handleAgentStream(payload));
  } catch {
    announce("Não foi possível iniciar o streaming do agente");
  }
}

function setLoginButtonsDisabled(disabled) {
  loginOptions.querySelectorAll("button").forEach((button) => { button.disabled = disabled; });
}

function handleAgentLogin(event) {
  if (!event?.type) return;
  if (event.type === "browser_opened") {
    integrationNote.textContent = "Conclua a autenticação na janela do navegador. O Lyrnova atualizará a conta automaticamente.";
  }
  if (event.type === "device_code") {
    integrationNote.textContent = `No navegador, acesse ${event.verificationUrl} e informe o código ${event.userCode}.`;
  }
  if (event.type === "completed") {
    renderAccount({ account: event.account });
    integrationNote.textContent = "Conta conectada com sucesso.";
    setLoginButtonsDisabled(false);
  }
  if (event.type === "failed") {
    integrationNote.textContent = event.code?.code === "unsafe_login_url"
      ? "O login foi bloqueado porque o endereço recebido não pertence à OpenAI."
      : "Não foi possível concluir o login. Tente novamente.";
    setLoginButtonsDisabled(false);
  }
  integrationNote.hidden = false;
  announce(integrationNote.textContent);
}

async function bindAgentLogin() {
  if (!listen) return;
  try {
    await listen("agent-login", ({ payload }) => handleAgentLogin(payload));
  } catch {
    announce("Não foi possível iniciar os eventos de autenticação");
  }
}

async function startAgentLogin(mode) {
  if (!aiProviderEnabled) {
    announce("Nenhum provider de IA está instalado e ativo");
    return;
  }
  if (!invoke) {
    integrationNote.textContent = "O login real está disponível somente no aplicativo desktop.";
    integrationNote.hidden = false;
    return;
  }
  setLoginButtonsDisabled(true);
  integrationNote.textContent = "Preparando autenticação segura…";
  integrationNote.hidden = false;
  try {
    await invoke("agent_login_start", { mode });
  } catch {
    integrationNote.textContent = "Já existe um login em andamento ou o provider está indisponível.";
    setLoginButtonsDisabled(false);
  }
}

async function sendAgentMessage(text) {
  if (!aiProviderEnabled) {
    announce("Nenhum provider de IA está instalado e ativo");
    return;
  }
  if (!invoke) {
    simulateAgentReply(text);
    return;
  }
  if (!currentProject) {
    transcript.append(createMessage("agent", "Abra ou crie um projeto antes de iniciar uma conversa de desenvolvimento."));
    announce("Nenhum projeto ativo");
    return;
  }
  setAgentBusy(true);
  try {
    const result = await invoke("agent_turn_start", {
      request: { threadId: activeAgentThreadId, prompt: text },
    });
    activeAgentThreadId = result.threadId;
  } catch (error) {
    setAgentBusy(false, "Indisponível");
    const unavailable = [
      "codex_unavailable",
      "provider_unavailable",
      "provider_unsupported",
      "provider_capability_unavailable",
    ].includes(error?.code);
    transcript.append(createMessage(
      "agent",
      unavailable
        ? "O provider de IA selecionado não está disponível para esta conversa."
        : "Não foi possível concluir a conversa. Verifique a conta e tente novamente.",
    ));
    announce("Falha ao conversar com o agente");
  }
}

function resetThread() {
  if (!aiProviderEnabled) {
    announce("Instale e ative um plugin de IA para criar conversas");
    return;
  }
  transcript.querySelectorAll(".message, .approval-card").forEach((item) => item.remove());
  streamedAgentMessages.clear();
  pendingApprovalCards.clear();
  activeAgentThreadId = null;
  setAgentBusy(false);
  transcript.append(createMessage("agent", currentProject
    ? "Nova conversa pronta. O agente começará em sandbox somente leitura e perguntará antes de ações sensíveis."
    : "Nova conversa pronta. Abra ou crie um projeto para começar."));
  prompt.focus();
  announce("Nova conversa criada");
}

function showChanges() {
  toggleInspector(true);
  document.querySelector('[data-tab="changes"]').click();
  closePalette();
}

document.addEventListener("click", (event) => {
  const actionButton = event.target.closest("[data-action]");
  if (actionButton) {
    const action = actionButton.dataset.action;
    if (action === "toggle-inspector") toggleInspector();
    if (action === "toggle-terminal") toggleTerminal();
    if (action === "close-terminal") toggleTerminal(false);
    if (action === "new-terminal") void startTerminal(true);
    if (action === "open-sidebar") appShell.dataset.sidebarOpen = "true";
    if (action === "close-sidebar") appShell.dataset.sidebarOpen = "false";
    if (action === "command-palette") openPalette();
    if (action === "new-thread") resetThread();
    if (action === "show-changes") showChanges();
    if (action === "open-account") openAccount();
    if (action === "close-account") closeAccount();
    if (action === "logout-account") void logoutAgentAccount();
    if (action === "return-to-agent") showWorkspaceView("agent");
    if (action === "show-agent-panel") showWorkspaceView("agent");
    if (action === "focus-editor") showWorkspaceView("editor", true);
    if (action === "save-document") void saveDocument();
    if (action === "new-file") void createWorkspaceDocument();
    if (action === "new-directory") void createWorkspaceDirectory();
    if (action === "move-entry") void moveWorkspaceEntry();
    if (action === "delete-entry") void deleteWorkspaceEntry();
    if (action === "restore-entry") void restoreWorkspaceEntry();
    if (action === "refresh-files") void loadWorkspaceTree(false);
    if (action === "refresh-git") void loadGitStatus();
    if (action === "refresh-tasks") void loadTasks();
    if (action === "cancel-task-review") void cancelTaskReview();
    if (action === "cancel-task") void cancelRunningTask();
    if (action === "git-commit") void commitGitChanges();
    if (action === "open-project") void openProjectDialog();
    if (action === "create-project") openCreateProjectDialog();
    if (action === "close-project-dialog") closeCreateProjectDialog();
    if (action === "select-plugin-package") void selectPluginPackage();
    if (action === "update-plugin-catalog") void updatePluginCatalog();
    if (action === "cancel-plugin-review") void cancelPluginReview();
    if (action === "cancel-plugin-removal") cancelPluginRemoval();
    if (action === "reset-settings") {
      ideSettings = { ...DEFAULT_IDE_SETTINGS };
      applyIdeSettings();
      announce("Configurações padrão restauradas");
    }
  }

  const settingAdjust = event.target.closest("[data-setting-adjust]");
  if (settingAdjust) {
    const key = settingAdjust.dataset.settingAdjust;
    const delta = Number(settingAdjust.dataset.delta);
    updateIdeSetting(key, Number(ideSettings[key]) + delta);
  }

  const settingsTarget = event.target.closest("[data-settings-target]");
  if (settingsTarget) {
    showWorkspaceView("settings");
    document.querySelector(`#${settingsTarget.dataset.settingsTarget}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
    if (narrowWorkspace.matches) appShell.dataset.sidebarOpen = "false";
  }

  const decision = event.target.closest("[data-decision]");
  if (decision) {
    const card = decision.closest(".approval-card");
    if (card) void resolveApproval(card, decision.dataset.decision);
  }

  const approvalRule = event.target.closest("[data-approval-rule-revoke]");
  if (approvalRule) {
    void revokeApprovalRule(approvalRule.dataset.approvalRuleRevoke, approvalRule);
  }

  const gitAction = event.target.closest("[data-git-action]");
  if (gitAction) void runGitFileAction(gitAction.dataset.gitAction, gitAction.dataset.gitPath);

  const pluginToggle = event.target.closest("[data-plugin-toggle]");
  if (pluginToggle) {
    void setPluginEnabled(pluginToggle.dataset.pluginToggle, pluginToggle.dataset.enable === "true");
  }

  const pluginRemove = event.target.closest("[data-plugin-remove]");
  if (pluginRemove) showPluginRemoval(pluginRemove.dataset.pluginRemove);

  const pluginDownload = event.target.closest("[data-plugin-download]");
  if (pluginDownload) void downloadPluginPackage(pluginDownload.dataset.pluginDownload);

  const task = event.target.closest("[data-task-plugin][data-task-id]");
  if (task) void showTaskReview(task.dataset.taskPlugin, task.dataset.taskId);

  const dock = event.target.closest("[data-dock-view]");
  if (dock) setDockView(dock.dataset.dockView);

  const tab = event.target.closest("[data-tab]");
  if (tab) {
    document.querySelectorAll("[data-tab]").forEach((item) => {
      const active = item === tab;
      item.classList.toggle("active", active);
      item.setAttribute("aria-selected", String(active));
    });
    document.querySelectorAll("[data-panel]").forEach((panel) => { panel.hidden = panel.dataset.panel !== tab.dataset.tab; });
  }

  const file = event.target.closest("[data-file]");
  if (file) {
    selectedWorkspacePath = file.dataset.file;
    void openDocument(file.dataset.file, true, Number.parseInt(file.dataset.line, 10) || null);
    if (fileFilter.value.trim()) void searchWorkspace();
    else renderWorkspaceTree();
  }

  const directory = event.target.closest("[data-directory]");
  if (directory) {
    const path = directory.dataset.directory;
    selectedWorkspacePath = path;
    if (fileFilter.value.trim()) {
      fileFilter.value = "";
      path.split("/").forEach((_, index, parts) => collapsedDirectories.delete(parts.slice(0, index + 1).join("/")));
    } else if (collapsedDirectories.has(path)) collapsedDirectories.delete(path);
    else collapsedDirectories.add(path);
    renderWorkspaceTree();
    announce(`${documentName(path)} ${collapsedDirectories.has(path) ? "recolhida" : "expandida"}`);
  }

  const activity = event.target.closest("[data-activity]");
  if (activity) switchActivity(activity.dataset.activity);

  const editorTab = event.target.closest("[data-editor-path]");
  if (editorTab) void openDocument(editorTab.dataset.editorPath);

  const closeEditorTab = event.target.closest("[data-close-editor-path]");
  if (closeEditorTab) void closeDocument(closeEditorTab.dataset.closeEditorPath);

  const command = event.target.closest("[data-command]");
  if (command) {
    if (command.dataset.command === "new-thread") resetThread();
    if (command.dataset.command === "toggle-terminal") toggleTerminal();
    if (command.dataset.command === "show-changes") showChanges();
    if (command.dataset.command === "open-account") openAccount();
    if (command.dataset.command === "open-editor") void openDocument(activeDocument);
    if (command.dataset.command === "open-project") void openProjectDialog();
    if (command.dataset.command === "create-project") openCreateProjectDialog();
    if (command.dataset.command === "open-settings") switchActivity("settings");
    closePalette();
  }

  const login = event.target.closest("[data-login]");
  if (login) {
    void startAgentLogin(login.dataset.login === "device" ? "device_code" : "browser");
  }
  if (event.target === palette) closePalette();
});

accountDialog.addEventListener("click", (event) => {
  if (event.target === accountDialog) closeAccount();
});

projectDialog.addEventListener("click", (event) => {
  if (event.target === projectDialog) closeCreateProjectDialog();
});

pluginReviewDialog.addEventListener("click", (event) => {
  if (event.target === pluginReviewDialog) void cancelPluginReview();
});

pluginReviewDialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  void cancelPluginReview();
});

pluginPermissionList.addEventListener("change", updatePluginReviewConfirmation);

pluginReviewForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void confirmPluginInstall();
});

pluginRemoveDialog.addEventListener("click", (event) => {
  if (event.target === pluginRemoveDialog) cancelPluginRemoval();
});

pluginRemoveDialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  cancelPluginRemoval();
});

pluginRemoveForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void confirmPluginRemoval();
});

taskReviewDialog.addEventListener("click", (event) => {
  if (event.target === taskReviewDialog) void cancelTaskReview();
});

taskReviewDialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  void cancelTaskReview();
});

taskReviewForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void executeReviewedTask();
});

projectForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void createProject();
});

terminalForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const command = terminalInput.value;
  if (!command || !invoke) return;
  appendTerminalOutput(`$ ${command}\n`);
  terminalInput.value = "";
  void invoke("terminal_write", { input: command }).catch(() => {
    appendTerminalOutput("[Falha ao enviar comando]\n");
  });
});

composer.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = prompt.value.trim();
  if (!text || agentTurnRunning || !aiProviderEnabled) return;
  transcript.append(createMessage("user", text));
  prompt.value = "";
  prompt.style.height = "auto";
  void sendAgentMessage(text);
});

prompt.addEventListener("input", () => {
  prompt.style.height = "auto";
  prompt.style.height = `${Math.min(prompt.scrollHeight, 180)}px`;
});

prompt.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); composer.requestSubmit(); }
});

fileFilter.addEventListener("input", scheduleWorkspaceSearch);
gitCommitMessage.addEventListener("input", updateGitCommitButton);
document.querySelectorAll("[data-setting]").forEach((control) => {
  control.addEventListener("change", () => {
    const value = control.type === "checkbox" ? control.checked : control.value;
    updateIdeSetting(control.dataset.setting, value);
  });
});

document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") { closePalette(); appShell.dataset.sidebarOpen = "false"; }
  if (event.ctrlKey && event.key.toLowerCase() === "k") { event.preventDefault(); openPalette(); }
  if (event.ctrlKey && event.key === ",") { event.preventDefault(); switchActivity("settings"); }
  if (event.ctrlKey && event.key.toLowerCase() === "n" && aiProviderEnabled) { event.preventDefault(); resetThread(); }
  if (event.ctrlKey && event.key.toLowerCase() === "o") { event.preventDefault(); void openProjectDialog(); }
  if (event.ctrlKey && event.key === "`") { event.preventDefault(); toggleTerminal(); }
  if (event.ctrlKey && event.key === "1") { event.preventDefault(); showWorkspaceView("editor", true); }
  if (event.ctrlKey && event.key === "2") { event.preventDefault(); void openDocument(activeDocument); }
  if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "e") { event.preventDefault(); switchActivity("explorer"); }
  if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "g") { event.preventDefault(); switchActivity("git"); }
  if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "a" && aiProviderEnabled) { event.preventDefault(); switchActivity("agent"); }
});

if (narrowWorkspace.matches) toggleInspector(false);
narrowWorkspace.addEventListener("change", (event) => {
  if (event.matches) toggleInspector(false);
});
systemDarkTheme.addEventListener("change", () => {
  if (ideSettings.appTheme === "system") applyIdeSettings(false);
});

initializeCodeEditor();
bindWindowControls();
bindPanelResizers();
void bindTerminalOutput();
void bindTaskOutput();
void bindProcessAudit();

function applyAiProviderAvailability(provider) {
  activeAiProvider = provider;
  const enabled = Boolean(provider);
  aiProviderEnabled = enabled;
  appShell.dataset.aiPluginEnabled = String(enabled);
  document.querySelectorAll(".ai-plugin-control").forEach((element) => {
    element.hidden = !enabled;
  });
  if (!enabled) {
    appShell.dataset.chatOpen = "false";
    conversation.classList.remove("visible");
    if (appShell.dataset.activityPanel === "agent") switchActivity("explorer");
    if (accountDialog.open) closeAccount();
  }
  window.requestAnimationFrame(() => codeEditor?.layout());
}

async function refreshAiProviderAvailability() {
  if (!invoke) {
    applyAiProviderAvailability(null);
    return false;
  }
  try {
    const provider = await invoke("ai_provider_current");
    applyAiProviderAvailability(provider);
    if (provider) {
      await bindAgentEvents();
      void loadAgentAccount();
    }
    return Boolean(provider);
  } catch {
    applyAiProviderAvailability(null);
    return false;
  }
}

async function loadPluginAvailability() {
  if (!invoke) {
    renderPluginCatalog([]);
    applyAiProviderAvailability(null);
    return false;
  }
  try {
    const [plugins, provider] = await Promise.all([
      invoke("plugin_list"),
      invoke("ai_provider_current"),
    ]);
    renderPluginCatalog(plugins);
    applyAiProviderAvailability(provider);
    return Boolean(provider);
  } catch {
    renderPluginCatalog([]);
    applyAiProviderAvailability(null);
    return false;
  }
}

async function loadTrustedPluginCatalog() {
  if (!invoke) {
    renderTrustedPluginCatalog([]);
    return;
  }
  try {
    renderTrustedPluginCatalog(await invoke("plugin_catalog_list"));
  } catch (error) {
    renderTrustedPluginCatalog([]);
    setPluginInstallStatus(pluginFlowErrorMessage(error), "error");
  }
}

async function bindAgentEvents() {
  if (agentEventsBound || !aiProviderEnabled) return;
  agentEventsBound = true;
  await Promise.all([bindAgentStream(), bindAgentLogin()]);
}

async function initializeWorkspace() {
  const [hasProject, hasAiProvider] = await Promise.all([
    loadProjectSummary(),
    loadPluginAvailability(),
    loadTrustedPluginCatalog(),
    loadTasks(),
  ]);
  renderTrustedPluginCatalog();
  if (hasProject) {
    await Promise.all([loadWorkspaceTree(), loadGitStatus()]);
    const firstFile = workspaceEntries.find((entry) => entry.kind === "file");
    if (firstFile) await openDocument(firstFile.path, false);
  }
  appShell.dataset.chatOpen = "false";
  if (hasAiProvider) {
    await bindAgentEvents();
    void loadAgentAccount();
  }
  if (hasProject && !terminal.hidden) void startTerminal();
}

void initializeWorkspace();
