use super::*;
use std::{
    io::Read,
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_PENDING: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    Hover,
    Definition,
    Completion,
    CompletionResolve,
    CodeAction,
    References,
    Rename,
    Formatting,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanguageQuery {
    pub request_id: String,
    pub kind: QueryKind,
    pub path: String,
    pub version: i32,
    pub position: Position,
    #[serde(default)]
    pub new_name: Option<String>,
    #[serde(default)]
    pub resolve_id: Option<String>,
    #[serde(default)]
    pub end: Option<Position>,
}

#[derive(Debug, Serialize)]
pub struct Definition {
    pub path: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ExternalSource>,
}
#[derive(Debug, Serialize)]
pub struct ExternalSource {
    pub label: String,
    pub content: String,
}
#[derive(Debug, Serialize)]
pub struct Hover {
    pub text: String,
    pub range: Option<Range>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum QueryResult {
    Hover(Option<Hover>),
    Definition(Vec<Definition>),
    References(Vec<Definition>),
    Completion(super::editing::Completions),
    CodeAction(Vec<super::editing::CodeAction>),
    Rename(Vec<super::editing::DocumentEdits>),
    Formatting(Vec<super::editing::TextEdit>),
}

#[derive(Default)]
pub(super) struct Queries {
    pub revision: u64,
    next_id: u64,
    pub hover: bool,
    pub definition: bool,
    pub completion: bool,
    pub completion_resolve: bool,
    pub code_action: bool,
    resolutions: BTreeMap<String, Resolution>,
    pub references: bool,
    pub rename: bool,
    pub formatting: bool,
    pending: BTreeMap<u64, Pending>,
}
struct Resolution {
    query: LanguageQuery,
    revision: u64,
    expires: Instant,
    raw: Value,
}
struct Pending {
    resolved: Option<Value>,
    query: LanguageQuery,
    revision: u64,
    deadline: Instant,
    sent: bool,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Result<Value, LanguageError>>,
}
pub struct QueryTicket {
    resolved: Option<Value>,
    baselines: BTreeMap<String, String>,
    query: LanguageQuery,
    revision: u64,
    cancelled: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Result<Value, LanguageError>>,
}
impl Drop for QueryTicket {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
impl QueryTicket {
    pub fn wait(&self) -> Result<Value, LanguageError> {
        self.receiver
            .recv_timeout(TIMEOUT + Duration::from_millis(250))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => LanguageError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => LanguageError::ServerExited,
            })?
    }
}
impl LanguageService {
    pub fn query(
        &self,
        root: &Path,
        id: &str,
        query: LanguageQuery,
    ) -> Result<QueryTicket, LanguageError> {
        if uuid::Uuid::parse_str(&query.request_id).is_err() {
            return Err(LanguageError::InvalidDocument);
        }
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        let current = matching(&session, root, id)?;
        let mut state = current
            .state
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        if state.snapshot.state != "running" {
            return Err(LanguageError::ServerUnavailable);
        }
        let doc = state
            .documents
            .get(&query.path)
            .ok_or(LanguageError::StaleDocument)?;
        if query.version != doc.version {
            return Err(LanguageError::StaleDocument);
        }
        if !valid_position(&doc.text, query.position) {
            return Err(LanguageError::InvalidDocument);
        }
        if query.kind == QueryKind::Rename {
            let name = query
                .new_name
                .as_deref()
                .ok_or(LanguageError::InvalidDocument)?;
            if name.is_empty() || name.len() > 256 || name.contains(['\0', '\n', '\r']) {
                return Err(LanguageError::InvalidDocument);
            }
        } else if query.new_name.is_some() {
            return Err(LanguageError::InvalidDocument);
        }
        if query.kind == QueryKind::CodeAction {
            valid_range(
                &doc.text,
                &json!({"start":query.position,"end":query.end.unwrap_or(query.position)}),
            )?;
        } else if query.end.is_some() {
            return Err(LanguageError::InvalidDocument);
        }
        let queries = &mut state.queries;
        if !match query.kind {
            QueryKind::Hover => queries.hover,
            QueryKind::Definition => queries.definition,
            QueryKind::Completion => queries.completion,
            QueryKind::CompletionResolve => queries.completion_resolve,
            QueryKind::CodeAction => queries.code_action,
            QueryKind::References => queries.references,
            QueryKind::Rename => queries.rename,
            QueryKind::Formatting => queries.formatting,
        } {
            return Err(LanguageError::UnsupportedFeature);
        }
        let resolved = if query.kind == QueryKind::CompletionResolve {
            let entry = query
                .resolve_id
                .as_ref()
                .and_then(|id| queries.resolutions.get(id))
                .ok_or(LanguageError::StaleDocument)?;
            if entry.revision != queries.revision
                || entry.expires <= Instant::now()
                || entry.query.path != query.path
                || entry.query.version != query.version
                || entry.query.position != query.position
            {
                return Err(LanguageError::StaleDocument);
            }
            Some(entry.raw.clone())
        } else {
            if query.resolve_id.is_some() {
                return Err(LanguageError::InvalidDocument);
            }
            None
        };
        if queries.pending.len() >= MAX_PENDING
            || queries
                .pending
                .values()
                .any(|p| p.query.request_id == query.request_id)
        {
            return Err(LanguageError::Busy);
        }
        queries.next_id = queries
            .next_id
            .checked_add(1)
            .ok_or(LanguageError::StateUnavailable)?;
        let wire_id = queries
            .next_id
            .checked_add(2)
            .ok_or(LanguageError::StateUnavailable)?; // initialize=1, shutdown=2
        let baselines = if matches!(query.kind, QueryKind::Rename | QueryKind::CodeAction) {
            super::editing::baselines(root)?
        } else {
            BTreeMap::new()
        };
        let (reply, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        queries.pending.insert(
            wire_id,
            Pending {
                resolved: resolved.clone(),
                query: query.clone(),
                revision: queries.revision,
                deadline: Instant::now() + TIMEOUT,
                sent: false,
                cancelled: cancelled.clone(),
                reply,
            },
        );
        Ok(QueryTicket {
            resolved,
            baselines,
            query,
            revision: queries.revision,
            cancelled,
            receiver,
        })
    }
    pub fn cancel_query(
        &self,
        root: &Path,
        id: &str,
        request_id: &str,
    ) -> Result<(), LanguageError> {
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        let current = matching(&session, root, id)?;
        let state = current
            .state
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        if let Some(pending) = state
            .queries
            .pending
            .values()
            .find(|p| p.query.request_id == request_id)
        {
            pending.cancelled.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub fn finish_query(
        &self,
        root: &Path,
        id: &str,
        ticket: &QueryTicket,
        value: Value,
    ) -> Result<QueryResult, LanguageError> {
        let session = self
            .session
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        let current = matching(&session, root, id)?;
        let mut state = current
            .state
            .lock()
            .map_err(|_| LanguageError::StateUnavailable)?;
        if state.queries.revision != ticket.revision {
            return Err(LanguageError::StaleDocument);
        }
        if ticket.cancelled.load(Ordering::Acquire) {
            return Err(LanguageError::Cancelled);
        }
        if state.snapshot.state != "running" {
            return Err(LanguageError::ServerExited);
        }
        let value = if let Some(original) = &ticket.resolved {
            // Only additionalTextEdits and plain detail may resolve. Insertion, range,
            // snippets and commands cannot change after presenting a suggestion.
            for key in [
                "label",
                "textEdit",
                "insertText",
                "insertTextFormat",
                "command",
            ] {
                if value.get(key) != original.get(key) {
                    return Err(LanguageError::ProtocolViolation);
                }
            }
            json!([value])
        } else {
            value
        };
        let mut result =
            parse_result(root, &ticket.query, value, &state.documents, &state.sources)?;
        let groups: Vec<&Vec<super::editing::DocumentEdits>> = match &result {
            QueryResult::Rename(edits) => vec![edits],
            QueryResult::CodeAction(actions) => actions.iter().map(|a| &a.edits).collect(),
            _ => vec![],
        };
        for edits in groups {
            for edit in edits {
                if !state.documents.contains_key(&edit.path)
                    && ticket.baselines.get(&edit.path) != Some(&edit.original)
                {
                    return Err(LanguageError::StaleDocument);
                }
            }
        }
        if ticket.query.kind == QueryKind::Completion {
            state.queries.cache_completions(&ticket.query, &mut result);
        }
        Ok(result)
    }
}

impl Queries {
    fn cache_completions(&mut self, query: &LanguageQuery, result: &mut QueryResult) {
        let QueryResult::Completion(completions) = result else {
            return;
        };
        // One bounded result set per session. Opaque IDs never expose raw server data.
        self.resolutions.clear();
        let mut bytes = 0;
        completions.items.retain_mut(|item| {
            let Some(raw) = item.raw.take() else {
                return true;
            };
            if !self.completion_resolve {
                completions.incomplete = true;
                return false;
            }
            bytes += raw.to_string().len();
            if bytes > 2 * 1024 * 1024 {
                completions.incomplete = true;
                return false;
            }
            let id = uuid::Uuid::new_v4().to_string();
            self.resolutions.insert(
                id.clone(),
                Resolution {
                    query: query.clone(),
                    revision: self.revision,
                    expires: Instant::now() + Duration::from_secs(120),
                    raw,
                },
            );
            item.resolve_id = Some(id);
            true
        });
    }

    // Called after document synchronization, in the same worker/output queue.
    pub fn messages(&mut self) -> Vec<Value> {
        let mut messages = Vec::new();
        self.pending.retain(|id, pending| {
            let error = if pending.cancelled.load(Ordering::Acquire) { Some(LanguageError::Cancelled) }
                else if pending.revision != self.revision { Some(LanguageError::StaleDocument) }
                else if Instant::now() >= pending.deadline { Some(LanguageError::Timeout) } else { None };
            if let Some(error) = error {
                if pending.sent { messages.push(json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":id}})); }
                let _ = pending.reply.send(Err(error));
                return false;
            }
            if !pending.sent {
                let method = match pending.query.kind {
                    QueryKind::Hover => "textDocument/hover", QueryKind::Definition => "textDocument/definition",
                    QueryKind::Completion => "textDocument/completion",
                    QueryKind::CompletionResolve => "completionItem/resolve",
                    QueryKind::CodeAction => "textDocument/codeAction", QueryKind::References => "textDocument/references",
                    QueryKind::Rename => "textDocument/rename", QueryKind::Formatting => "textDocument/formatting",
                };
                let mut params = json!({"textDocument":{"uri":uri(&pending.query.path)}, "position":pending.query.position});
                match pending.query.kind {
                    QueryKind::CompletionResolve => params = pending.resolved.clone().expect("validated resolution"),
                    QueryKind::CodeAction => {
                        params.as_object_mut().unwrap().remove("position");
                        params["range"] = json!({"start":pending.query.position,"end":pending.query.end.unwrap_or(pending.query.position)});
                        params["context"] = json!({"diagnostics":[],"only":["quickfix"]});
                    },
                    QueryKind::References => params["context"] = json!({"includeDeclaration":true}),
                    QueryKind::Rename => params["newName"] = json!(pending.query.new_name),
                    QueryKind::Formatting => { params.as_object_mut().unwrap().remove("position"); params["options"] = json!({"tabSize":4,"insertSpaces":true}); },
                    _ => {},
                }
                messages.push(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}));
                pending.sent = true;
            }
            true
        });
        messages
    }
    pub fn respond(&mut self, id: u64, message: &Value) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        let result = if pending.cancelled.load(Ordering::Acquire) {
            Err(LanguageError::Cancelled)
        } else if pending.revision != self.revision {
            Err(LanguageError::StaleDocument)
        } else if Instant::now() >= pending.deadline {
            Err(LanguageError::Timeout)
        } else if message.get("error").is_some() {
            Err(LanguageError::UnsupportedFeature)
        } else {
            message
                .get("result")
                .cloned()
                .ok_or(LanguageError::ProtocolViolation)
        };
        let _ = pending.reply.send(result);
    }
    pub fn stop(&mut self) {
        self.pending.clear();
        self.resolutions.clear();
    }
}

pub(super) fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, LanguageError> {
    if !relative.ends_with(".rs")
        || relative.len() > 4096
        || relative.contains(['\0', '\\'])
        || relative
            .split('/')
            .any(|p| p.is_empty() || matches!(p, "." | ".." | ".git"))
    {
        return Err(LanguageError::InvalidDocument);
    }
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => return Err(LanguageError::InvalidDocument),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(LanguageError::InvalidDocument);
            }
            _ => {}
        }
    }
    Ok(path)
}
fn valid_position(text: &str, position: Position) -> bool {
    let Some(line) = text.split('\n').nth(position.line as usize) else {
        return false;
    };
    let line = line.trim_end_matches('\r');
    let mut column = 0;
    for ch in line.chars() {
        if column == position.character {
            return true;
        }
        column += ch.len_utf16() as u32;
    }
    column == position.character
}
pub(super) fn valid_range(text: &str, value: &Value) -> Result<Range, LanguageError> {
    let range: Range =
        serde_json::from_value(value.clone()).map_err(|_| LanguageError::ProtocolViolation)?;
    if range.start > range.end
        || !valid_position(text, range.start)
        || !valid_position(text, range.end)
    {
        return Err(LanguageError::ProtocolViolation);
    }
    Ok(range)
}
pub(super) fn target_path(raw: &str) -> Result<String, LanguageError> {
    let url = Url::parse(raw).map_err(|_| LanguageError::InvalidDocument)?;
    let path = url
        .to_file_path()
        .map_err(|_| LanguageError::InvalidDocument)?;
    let relative = path
        .strip_prefix("/workspace")
        .map_err(|_| LanguageError::InvalidDocument)?;
    let relative = relative.to_str().ok_or(LanguageError::InvalidDocument)?;
    if uri(relative) != raw {
        return Err(LanguageError::InvalidDocument);
    }
    Ok(relative.into())
}
fn open_target(root: &Path, relative: &str) -> Result<std::fs::File, LanguageError> {
    checked_path(root, relative)?;
    #[cfg(unix)]
    {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::OpenOptionsExt,
        };
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(root)
            .map_err(|_| LanguageError::InvalidDocument)?;
        let mut parts = relative.split('/').peekable();
        while let Some(part) = parts.next() {
            let name = std::ffi::CString::new(part).map_err(|_| LanguageError::InvalidDocument)?;
            let flags = libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | if parts.peek().is_some() {
                    libc::O_DIRECTORY
                } else {
                    libc::O_NONBLOCK
                };
            // SAFETY: the parent descriptor and NUL-terminated component remain valid.
            let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err(LanguageError::InvalidDocument);
            }
            // SAFETY: openat returned a new owned descriptor; previous parents can now close.
            file = unsafe { std::fs::File::from_raw_fd(fd) };
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        Err(LanguageError::UnsupportedPlatform)
    }
}
pub(super) fn read_target(root: &Path, relative: &str) -> Result<String, LanguageError> {
    let file = open_target(root, relative)?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MAX_DOCUMENT as u64)
    {
        return Err(LanguageError::InvalidDocument);
    }
    let mut text = String::new();
    file.take(MAX_DOCUMENT as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| LanguageError::InvalidDocument)?;
    if text.len() > MAX_DOCUMENT || text.contains('\0') {
        return Err(LanguageError::TooLarge);
    }
    Ok(text)
}
fn parse_result(
    root: &Path,
    query: &LanguageQuery,
    value: Value,
    docs: &BTreeMap<String, Arc<LanguageDocument>>,
    sources: &[super::environment::SourceRoot],
) -> Result<QueryResult, LanguageError> {
    match query.kind {
        QueryKind::Hover => {
            if value.is_null() {
                return Ok(QueryResult::Hover(None));
            }
            let content = value
                .get("contents")
                .ok_or(LanguageError::ProtocolViolation)?;
            let parts = if let Some(parts) = content.as_array() {
                parts.clone()
            } else {
                vec![content.clone()]
            };
            if parts.len() > 32 {
                return Err(LanguageError::TooLarge);
            }
            let mut text = String::new();
            for part in parts {
                let part = part
                    .as_str()
                    .or_else(|| part.get("value").and_then(Value::as_str))
                    .ok_or(LanguageError::ProtocolViolation)?;
                if text.len() + part.len() + 2 > 64 * 1024 || part.contains('\0') {
                    return Err(LanguageError::TooLarge);
                }
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(part);
            }
            let doc = docs.get(&query.path).ok_or(LanguageError::StaleDocument)?;
            let range = value
                .get("range")
                .map(|v| valid_range(&doc.text, v))
                .transpose()?;
            Ok(QueryResult::Hover(Some(Hover { text, range })))
        }
        QueryKind::Completion
        | QueryKind::CompletionResolve
        | QueryKind::CodeAction
        | QueryKind::Rename
        | QueryKind::Formatting => super::editing::parse(root, query, value, docs),
        QueryKind::Definition | QueryKind::References => {
            let entries = if value.is_null() {
                vec![]
            } else if let Some(entries) = value.as_array() {
                entries.clone()
            } else {
                vec![value]
            };
            if entries.len()
                > if query.kind == QueryKind::References {
                    256
                } else {
                    32
                }
            {
                return Err(LanguageError::TooLarge);
            }
            let mut results = Vec::new();
            let mut texts = BTreeMap::new();
            let mut bytes = 0;
            for entry in entries {
                let raw = entry
                    .get("uri")
                    .or_else(|| entry.get("targetUri"))
                    .and_then(Value::as_str)
                    .ok_or(LanguageError::ProtocolViolation)?;
                let (path, external) = match target_path(raw) {
                    Ok(path) => {
                        checked_path(root, &path)?;
                        (path, None)
                    }
                    Err(_) => {
                        let url = Url::parse(raw).map_err(|_| LanguageError::InvalidDocument)?;
                        let absolute = url
                            .to_file_path()
                            .map_err(|_| LanguageError::InvalidDocument)?;
                        if Url::from_file_path(&absolute)
                            .map_err(|_| LanguageError::InvalidDocument)?
                            .as_str()
                            != raw
                        {
                            return Err(LanguageError::InvalidDocument);
                        }
                        let (index, source) = sources
                            .iter()
                            .enumerate()
                            .find(|(_, source)| absolute.starts_with(&source.sandbox))
                            .ok_or(LanguageError::InvalidDocument)?;
                        let relative = absolute
                            .strip_prefix(&source.sandbox)
                            .ok()
                            .and_then(|p| p.to_str())
                            .ok_or(LanguageError::InvalidDocument)?;
                        let content = read_target(&source.host, relative)?;
                        (
                            format!("library:{index}/{relative}"),
                            Some(ExternalSource {
                                label: relative.into(),
                                content,
                            }),
                        )
                    }
                };
                if !texts.contains_key(&path) {
                    let text = if let Some(source) = &external {
                        source.content.clone()
                    } else {
                        match docs.get(&path) {
                            Some(doc) => doc.text.clone(),
                            None => read_target(root, &path)?,
                        }
                    };
                    bytes += text.len();
                    if bytes > MAX_TOTAL {
                        return Err(LanguageError::TooLarge);
                    }
                    texts.insert(path.clone(), text);
                }
                let text = &texts[&path];
                let range = valid_range(
                    text,
                    entry
                        .get("range")
                        .or_else(|| entry.get("targetSelectionRange"))
                        .ok_or(LanguageError::ProtocolViolation)?,
                )?;
                if let Some(target) = entry.get("targetRange") {
                    let target = valid_range(text, target)?;
                    if range.start < target.start || range.end > target.end {
                        return Err(LanguageError::ProtocolViolation);
                    }
                }
                if let Some(source) = &external {
                    bytes += source.content.len();
                    if bytes > MAX_TOTAL {
                        return Err(LanguageError::TooLarge);
                    }
                }
                results.push(Definition {
                    path,
                    range,
                    source: external,
                });
            }
            Ok(if query.kind == QueryKind::References {
                QueryResult::References(results)
            } else {
                QueryResult::Definition(results)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_result(
        root: &Path,
        query: &LanguageQuery,
        value: Value,
        docs: &BTreeMap<String, Arc<LanguageDocument>>,
    ) -> Result<QueryResult, LanguageError> {
        super::parse_result(root, query, value, docs, &[])
    }
    struct Fixture {
        root: PathBuf,
        service: LanguageService,
        state: Arc<Mutex<State>>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("lyrnova-query-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(root.join("src")).unwrap();
            let state = Arc::new(Mutex::new(State {
                sources: vec![],
                configuration: configuration(),
                queries: Queries {
                    hover: true,
                    definition: true,
                    ..Default::default()
                },
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
            let service = LanguageService {
                environments: Mutex::default(),
                session: Mutex::new(Some(Session {
                    root: root.clone(),
                    state: state.clone(),
                    cancel: Arc::new(AtomicBool::new(false)),
                    worker: None,
                })),
            };
            service
                .sync(
                    &root,
                    "session",
                    vec![LanguageDocument {
                        path: "src/lib.rs".into(),
                        version: 1,
                        text: "// 🦀\npub fn answer() {}".into(),
                    }],
                )
                .unwrap();
            Self {
                root,
                service,
                state,
            }
        }
        fn query(&self, kind: QueryKind) -> LanguageQuery {
            LanguageQuery {
                request_id: uuid::Uuid::new_v4().to_string(),
                kind,
                new_name: None,
                resolve_id: None,
                end: None,
                path: "src/lib.rs".into(),
                version: 1,
                position: Position {
                    line: 1,
                    character: 8,
                },
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn range(start: u32, end: u32) -> Value {
        json!({"start":{"line":0,"character":start},"end":{"line":0,"character":end}})
    }

    fn completion_token(f: &Fixture) -> (String, Value) {
        {
            let mut state = f.state.lock().unwrap();
            state.queries.completion = true;
            state.queries.completion_resolve = true;
        }
        let q = f.query(QueryKind::Completion);
        let ticket = f.service.query(&f.root, "session", q).unwrap();
        let raw = json!({"label":"answer","textEdit":{"range":{"start":{"line":1,"character":7},"end":{"line":1,"character":13}},"newText":"answer"},"data":{"imports":["secret-server-data"]}});
        let result = f
            .service
            .finish_query(&f.root, "session", &ticket, json!([raw]))
            .unwrap();
        let encoded = serde_json::to_value(&result).unwrap();
        assert!(!encoded.to_string().contains("secret-server-data"));
        (
            encoded["value"]["items"][0]["resolveId"]
                .as_str()
                .unwrap()
                .into(),
            raw,
        )
    }
    #[test]
    fn completion_resolution_is_opaque_bound_and_preserves_the_presented_insertion() {
        let f = Fixture::new();
        let (id, mut raw) = completion_token(&f);
        let mut q = f.query(QueryKind::CompletionResolve);
        q.resolve_id = Some(id);
        let ticket = f.service.query(&f.root, "session", q.clone()).unwrap();
        raw["additionalTextEdits"] =
            json!([{"range":range(0,0),"newText":"use crate::helper::answer;\n"}]);
        let resolved = f
            .service
            .finish_query(&f.root, "session", &ticket, raw.clone())
            .unwrap();
        assert!(
            serde_json::to_value(resolved).unwrap()["value"]["items"][0]["additional"]
                .as_array()
                .unwrap()
                .len()
                == 1
        );
        for key in ["command", "textEdit", "label", "insertTextFormat"] {
            let mut changed = raw.clone();
            changed[key] = json!("injected");
            assert!(
                f.service
                    .finish_query(&f.root, "session", &ticket, changed)
                    .is_err()
            );
        }
        raw["additionalTextEdits"] = json!([{"range":{"start":{"line":1,"character":7},"end":{"line":1,"character":8}},"newText":"overlap"}]);
        assert!(
            f.service
                .finish_query(&f.root, "session", &ticket, raw)
                .is_err()
        );
        q.position.character += 1;
        assert!(matches!(
            f.service.query(&f.root, "session", q.clone()),
            Err(LanguageError::StaleDocument)
        ));
        q.position.character -= 1;
        q.resolve_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(matches!(
            f.service.query(&f.root, "session", q),
            Err(LanguageError::StaleDocument)
        ));
    }
    #[test]
    fn completion_resolution_expires_and_invalidates_on_disk_revision_and_stop() {
        for reason in ["expiry", "revision", "stop"] {
            let f = Fixture::new();
            let (id, _) = completion_token(&f);
            let mut q = f.query(QueryKind::CompletionResolve);
            q.resolve_id = Some(id.clone());
            {
                let mut state = f.state.lock().unwrap();
                match reason {
                    "expiry" => {
                        state.queries.resolutions.get_mut(&id).unwrap().expires = Instant::now()
                    }
                    "revision" => state.queries.revision += 1,
                    _ => state.queries.stop(),
                }
            }
            assert!(matches!(
                f.service.query(&f.root, "session", q),
                Err(LanguageError::StaleDocument)
            ));
        }
    }
    #[test]
    fn quick_fix_refuses_changed_closed_file_and_sends_only_the_closed_method() {
        let f = Fixture::new();
        f.state.lock().unwrap().queries.code_action = true;
        std::fs::write(f.root.join("src/other.rs"), "fn before() {}\n").unwrap();
        let q = f.query(QueryKind::CodeAction);
        let ticket = f.service.query(&f.root, "session", q).unwrap();
        let messages = f.state.lock().unwrap().queries.messages();
        assert_eq!(messages[0]["method"], "textDocument/codeAction");
        assert_eq!(
            messages[0]["params"]["context"]["only"],
            json!(["quickfix"])
        );
        std::fs::write(f.root.join("src/other.rs"), "fn edited() {}\n").unwrap();
        let raw = json!([{"title":"Fix","kind":"quickfix","edit":{"changes":{"file:///workspace/src/other.rs":[{"range":range(3,9),"newText":"after"}]}}}]);
        assert!(matches!(
            f.service.finish_query(&f.root, "session", &ticket, raw),
            Err(LanguageError::StaleDocument)
        ));
    }

    #[test]
    fn positions_reject_surrogate_splits_stale_versions_and_unadvertised_features() {
        let f = Fixture::new();
        let mut q = f.query(QueryKind::Hover);
        q.position = Position {
            line: 0,
            character: 4,
        }; // middle of the crab surrogate pair
        assert!(matches!(
            f.service.query(&f.root, "session", q.clone()),
            Err(LanguageError::InvalidDocument)
        ));
        q.position.character = 5;
        assert!(f.service.query(&f.root, "session", q.clone()).is_ok());
        q.version = 2;
        assert!(matches!(
            f.service.query(&f.root, "session", q.clone()),
            Err(LanguageError::StaleDocument)
        ));
        q.version = 1;
        f.state.lock().unwrap().queries.hover = false;
        assert!(matches!(
            f.service.query(&f.root, "session", q.clone()),
            Err(LanguageError::UnsupportedFeature)
        ));
        assert!(matches!(
            f.service.query(&f.root, "old", q),
            Err(LanguageError::StaleSession)
        ));
        assert!(serde_json::from_value::<LanguageQuery>(json!({"requestId":"a", "kind":"execute_command", "path":"src/lib.rs", "version":1, "position":{"line":0,"character":0}})).is_err());
    }

    #[test]
    fn request_ids_route_out_of_order_and_late_results_are_ignored() {
        let f = Fixture::new();
        let first = f
            .service
            .query(&f.root, "session", f.query(QueryKind::Hover))
            .unwrap();
        let second = f
            .service
            .query(&f.root, "session", f.query(QueryKind::Definition))
            .unwrap();
        let mut state = f.state.lock().unwrap();
        let messages = state.queries.messages();
        assert_eq!(messages[0]["method"], "textDocument/hover");
        assert_eq!(messages[1]["method"], "textDocument/definition");
        assert!(state.queries.messages().is_empty());
        state.queries.respond(4, &json!({"result":[]}));
        state
            .queries
            .respond(3, &json!({"result":{"contents":"type"}}));
        state.queries.respond(3, &json!({"result":"late"}));
        drop(state);
        assert_eq!(second.wait().unwrap(), json!([]));
        assert_eq!(first.wait().unwrap(), json!({"contents":"type"}));
    }

    #[test]
    fn bounded_queue_cancel_timeout_and_document_changes_release_waiters() {
        let f = Fixture::new();
        let tickets: Vec<_> = (0..MAX_PENDING)
            .map(|_| {
                f.service
                    .query(&f.root, "session", f.query(QueryKind::Hover))
                    .unwrap()
            })
            .collect();
        assert!(matches!(
            f.service
                .query(&f.root, "session", f.query(QueryKind::Hover)),
            Err(LanguageError::Busy)
        ));
        assert_eq!(
            f.state.lock().unwrap().queries.messages().len(),
            MAX_PENDING
        );
        f.service
            .cancel_query(&f.root, "session", &tickets[0].query.request_id)
            .unwrap();
        {
            let mut state = f.state.lock().unwrap();
            state.queries.pending.get_mut(&4).unwrap().deadline =
                Instant::now() - Duration::from_secs(1);
            let messages = state.queries.messages();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0]["method"], "$/cancelRequest");
        }
        assert_eq!(tickets[0].wait(), Err(LanguageError::Cancelled));
        assert_eq!(tickets[1].wait(), Err(LanguageError::Timeout));
        f.service.sync(&f.root, "session", vec![]).unwrap();
        assert_eq!(
            f.state.lock().unwrap().queries.messages().len(),
            MAX_PENDING - 2
        );
        assert_eq!(tickets[2].wait(), Err(LanguageError::StaleDocument));
        assert!(matches!(
            f.service
                .finish_query(&f.root, "session", &tickets[2], Value::Null),
            Err(LanguageError::StaleDocument)
        ));
    }

    #[test]
    fn stop_disconnects_pending_requests_without_waiting_for_server() {
        let f = Fixture::new();
        let ticket = f
            .service
            .query(&f.root, "session", f.query(QueryKind::Hover))
            .unwrap();
        f.state.lock().unwrap().queries.stop();
        assert_eq!(ticket.wait(), Err(LanguageError::ServerExited));
        f.service.stop().unwrap();
        assert!(matches!(
            f.service
                .finish_query(&f.root, "session", &ticket, Value::Null),
            Err(LanguageError::StaleSession)
        ));
    }

    #[test]
    fn definitions_validate_locations_links_utf16_and_unopened_workspace_files() {
        let f = Fixture::new();
        let query = f.query(QueryKind::Definition);
        std::fs::write(f.root.join("src/a space.rs"), "// 🦀\npub fn target() {}").unwrap();
        let state = f.state.lock().unwrap();
        let result = parse_result(&f.root, &query, json!([
            {"uri":uri("src/a space.rs"),"range":range(3,5)},
            {"targetUri":uri("src/lib.rs"),"targetRange":range(0,5),"targetSelectionRange":range(3,5)}
        ]), &state.documents).unwrap();
        let QueryResult::Definition(locations) = result else {
            panic!()
        };
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].path, "src/a space.rs");
        for raw in [
            "file:///etc/passwd",
            "https://example.invalid/a.rs",
            "command:run",
            "file:///workspace/../a.rs",
            "file:///workspace/.git/a.rs",
            "file:///workspace/src/lib.rs#x",
        ] {
            assert!(
                parse_result(
                    &f.root,
                    &query,
                    json!({"uri":raw,"range":range(0,0)}),
                    &state.documents
                )
                .is_err()
            );
        }
        std::os::unix::fs::symlink(f.root.join("src/lib.rs"), f.root.join("src/link.rs")).unwrap();
        assert!(
            parse_result(
                &f.root,
                &query,
                json!({"uri":uri("src/link.rs"),"range":range(0,0)}),
                &state.documents
            )
            .is_err()
        );
        for invalid in [range(4, 5), range(0, 99), range(3, 0)] {
            assert!(
                parse_result(
                    &f.root,
                    &query,
                    json!({"uri":uri("src/lib.rs"),"range":invalid}),
                    &state.documents
                )
                .is_err()
            );
        }
        assert!(parse_result(&f.root, &query, json!({"targetUri":uri("src/lib.rs"),"targetRange":range(0,3),"targetSelectionRange":range(3,5)}), &state.documents).is_err());
    }

    #[test]
    fn external_definitions_require_an_authorized_source_root_and_return_read_only_content() {
        let f = Fixture::new();
        let library = f.root.join("library");
        std::fs::create_dir(&library).unwrap();
        std::fs::write(library.join("lib.rs"), "pub fn external() {}\n").unwrap();
        let query = f.query(QueryKind::Definition);
        let state = f.state.lock().unwrap();
        let value = json!({"uri":"file:///toolchain/library/lib.rs","range":range(7,15)});
        assert!(
            super::parse_result(&f.root, &query, value.clone(), &state.documents, &[]).is_err()
        );
        let sources = [super::super::environment::SourceRoot {
            host: library.clone(),
            sandbox: "/toolchain/library".into(),
        }];
        let QueryResult::Definition(results) =
            super::parse_result(&f.root, &query, value, &state.documents, &sources).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            results[0].source.as_ref().unwrap().content,
            "pub fn external() {}\n"
        );
        assert_eq!(results[0].source.as_ref().unwrap().label, "lib.rs");
        for raw in [
            "file:///tmp/cargo/credentials.rs",
            "file:///toolchain/library/../secret.rs",
            "file:///toolchain/library/lib.rs#fragment",
        ] {
            assert!(
                super::parse_result(
                    &f.root,
                    &query,
                    json!({"uri":raw,"range":range(0,0)}),
                    &state.documents,
                    &sources
                )
                .is_err()
            );
        }
        std::os::unix::fs::symlink(f.root.join("secret.rs"), library.join("link.rs")).unwrap();
        assert!(
            super::parse_result(
                &f.root,
                &query,
                json!({"uri":"file:///toolchain/library/link.rs","range":range(0,0)}),
                &state.documents,
                &sources
            )
            .is_err()
        );
    }

    #[test]
    fn hover_preserves_literal_untrusted_text_and_bounds_content_and_ranges() {
        let f = Fixture::new();
        let query = f.query(QueryKind::Hover);
        let state = f.state.lock().unwrap();
        let malicious = "[run](command:evil) <img src='https://example.invalid'>";
        for content in [
            json!(malicious),
            json!({"kind":"markdown","value":malicious}),
            json!([{"language":"rust","value":malicious}]),
        ] {
            let QueryResult::Hover(Some(hover)) = parse_result(
                &f.root,
                &query,
                json!({"contents":content,"range":range(3,5)}),
                &state.documents,
            )
            .unwrap() else {
                panic!()
            };
            assert_eq!(hover.text, malicious);
        }
        assert!(
            parse_result(
                &f.root,
                &query,
                json!({"contents":"x".repeat(65537)}),
                &state.documents
            )
            .is_err()
        );
        assert!(
            parse_result(
                &f.root,
                &query,
                json!({"contents":"type","range":range(4,5)}),
                &state.documents
            )
            .is_err()
        );
        assert!(matches!(
            parse_result(&f.root, &query, Value::Null, &state.documents),
            Ok(QueryResult::Hover(None))
        ));
    }
    #[test]
    fn rename_rejects_closed_files_changed_while_request_was_pending() {
        let f = Fixture::new();
        f.state.lock().unwrap().queries.rename = true;
        std::fs::write(f.root.join("src/helper.rs"), "fn old() {}\n").unwrap();
        let mut query = f.query(QueryKind::Rename);
        query.new_name = Some("new".into());
        let ticket = f.service.query(&f.root, "session", query).unwrap();
        std::fs::write(f.root.join("src/helper.rs"), "fn changed() {}\n").unwrap();
        let result = f.service.finish_query(&f.root, "session", &ticket, json!({"changes":{"file:///workspace/src/helper.rs":[{"range":range(3,6),"newText":"new"}]}}));
        assert!(matches!(result, Err(LanguageError::StaleDocument)));
    }
    #[test]
    fn disk_invalidation_rejects_pending_queries_without_losing_open_drafts() {
        let f = Fixture::new();
        let query = f.query(QueryKind::Hover);
        let ticket = f.service.query(&f.root, "session", query).unwrap();
        f.service.invalidate_disk(&f.root.join("other")).unwrap();
        assert_eq!(f.state.lock().unwrap().snapshot.state, "running");
        f.service.invalidate_disk(&f.root).unwrap();
        assert_eq!(f.state.lock().unwrap().snapshot.state, "outdated");
        assert_eq!(f.state.lock().unwrap().documents["src/lib.rs"].version, 1);
        assert!(matches!(
            f.service
                .finish_query(&f.root, "session", &ticket, json!({"contents":"old"})),
            Err(LanguageError::StaleDocument)
        ));
        assert!(matches!(ticket.wait(), Err(LanguageError::ServerExited)));
    }
}
