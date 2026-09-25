use super::{
    query::{self, LanguageQuery, QueryKind, QueryResult},
    *,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}
#[derive(Debug, Serialize)]
pub struct DocumentEdits {
    pub path: String,
    pub original: String,
    pub edits: Vec<TextEdit>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    label: String,
    detail: String,
    kind: u64,
    insert_text: String,
    snippet: bool,
    range: Option<Range>,
    additional: Vec<TextEdit>,
    sort_text: String,
    filter_text: String,
}
#[derive(Debug, Serialize)]
pub struct Completions {
    items: Vec<Completion>,
    incomplete: bool,
}

// Snapshot closed Rust sources before rename; reject oversized workspaces instead of
// accepting edits whose input was never observed. No symlink traversal.
pub(super) fn baselines(root: &Path) -> Result<BTreeMap<String, String>, LanguageError> {
    let mut files = BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    let mut visited = 0;
    let mut bytes = 0;
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).map_err(|_| LanguageError::InvalidDocument)? {
            let entry = entry.map_err(|_| LanguageError::InvalidDocument)?;
            visited += 1;
            if visited > 50_000 {
                return Err(LanguageError::TooLarge);
            }
            let kind = entry
                .file_type()
                .map_err(|_| LanguageError::InvalidDocument)?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | "node_modules")
                ) {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .ok_or(LanguageError::InvalidDocument)?;
                let text = query::read_target(root, relative)?;
                bytes += text.len();
                if files.len() >= 512 || bytes > MAX_TOTAL {
                    return Err(LanguageError::TooLarge);
                }
                files.insert(relative.into(), text);
            }
        }
    }
    Ok(files)
}
fn string(value: &Value, limit: usize) -> Result<String, LanguageError> {
    let text = value.as_str().ok_or(LanguageError::ProtocolViolation)?;
    if text.len() > limit || text.contains('\0') {
        return Err(LanguageError::TooLarge);
    }
    Ok(text.into())
}
fn edits(text: &str, value: &Value) -> Result<Vec<TextEdit>, LanguageError> {
    let entries = value.as_array().ok_or(LanguageError::ProtocolViolation)?;
    if entries.len() > 2048 {
        return Err(LanguageError::TooLarge);
    }
    let mut result = Vec::new();
    let mut bytes = 0;
    for entry in entries {
        // Annotated changes require a separate confirmation flow, not implicit acceptance.
        if entry.get("annotationId").is_some() {
            return Err(LanguageError::UnsupportedFeature);
        }
        let range = query::valid_range(text, &entry["range"])?;
        let new_text = string(&entry["newText"], MAX_DOCUMENT)?;
        bytes += new_text.len();
        if bytes > MAX_DOCUMENT {
            return Err(LanguageError::TooLarge);
        }
        result.push(TextEdit { range, new_text });
    }
    result.sort_by_key(|edit| (edit.range.start, edit.range.end));
    for pair in result.windows(2) {
        if pair[0].range.end > pair[1].range.start || pair[0].range.start == pair[1].range.start {
            return Err(LanguageError::ProtocolViolation);
        }
    }
    // Conservative upper bound keeps the resulting draft within synchronization limits.
    if text.len() + bytes > MAX_DOCUMENT {
        return Err(LanguageError::TooLarge);
    }
    Ok(result)
}
pub(super) fn parse(
    root: &Path,
    query: &LanguageQuery,
    value: Value,
    docs: &BTreeMap<String, Arc<LanguageDocument>>,
) -> Result<QueryResult, LanguageError> {
    let doc = docs.get(&query.path).ok_or(LanguageError::StaleDocument)?;
    match query.kind {
        QueryKind::Formatting => Ok(QueryResult::Formatting(if value.is_null() {
            vec![]
        } else {
            edits(&doc.text, &value)?
        })),
        QueryKind::Rename => {
            if value.is_null() {
                return Ok(QueryResult::Rename(vec![]));
            }
            if !value.is_object() || value.get("changeAnnotations").is_some() {
                return Err(LanguageError::UnsupportedFeature);
            }
            let mut changes = BTreeMap::new();
            if let Some(raw) = value.get("changes") {
                for (uri, edits) in raw.as_object().ok_or(LanguageError::ProtocolViolation)? {
                    changes.insert(query::target_path(uri)?, edits.clone());
                }
            }
            if let Some(raw) = value.get("documentChanges") {
                if value.get("changes").is_some() {
                    return Err(LanguageError::ProtocolViolation);
                }
                for change in raw.as_array().ok_or(LanguageError::ProtocolViolation)? {
                    if change.get("kind").is_some() {
                        return Err(LanguageError::UnsupportedFeature);
                    }
                    let path = query::target_path(
                        change["textDocument"]["uri"]
                            .as_str()
                            .ok_or(LanguageError::ProtocolViolation)?,
                    )?;
                    let version = &change["textDocument"]["version"];
                    if !version.is_null()
                        && docs.get(&path).map(|doc| i64::from(doc.version)) != version.as_i64()
                    {
                        return Err(LanguageError::StaleDocument);
                    }
                    if changes.insert(path, change["edits"].clone()).is_some() {
                        return Err(LanguageError::ProtocolViolation);
                    }
                }
            }
            if changes.len() > MAX_DOCUMENTS {
                return Err(LanguageError::TooLarge);
            }
            let mut result = Vec::new();
            let mut bytes = 0;
            for (path, raw) in changes {
                query::checked_path(root, &path)?;
                let original = match docs.get(&path) {
                    Some(doc) => doc.text.clone(),
                    None => query::read_target(root, &path)?,
                };
                let edits = edits(&original, &raw)?;
                bytes +=
                    original.len() + edits.iter().map(|edit| edit.new_text.len()).sum::<usize>();
                if bytes > MAX_TOTAL {
                    return Err(LanguageError::TooLarge);
                }
                result.push(DocumentEdits {
                    path,
                    original,
                    edits,
                });
            }
            Ok(QueryResult::Rename(result))
        }
        QueryKind::Completion => {
            let empty = vec![];
            let entries = if value.is_null() {
                &empty
            } else {
                value
                    .as_array()
                    .or_else(|| value["items"].as_array())
                    .ok_or(LanguageError::ProtocolViolation)?
            };
            let mut items = Vec::new();
            for item in entries.iter().take(256) {
                // Completion resolution/commands are intentionally not executable IPC.
                // Auto-import is disabled in config; eager same-document edits are supported.
                if item.get("command").is_some() {
                    continue;
                }
                let label = string(&item["label"], 4096)?;
                let detail = item
                    .get("detail")
                    .map(|v| string(v, 16384))
                    .transpose()?
                    .unwrap_or_default();
                let edit = item.get("textEdit");
                let insert_text = string(
                    edit.map(|e| &e["newText"])
                        .unwrap_or_else(|| item.get("insertText").unwrap_or(&item["label"])),
                    65536,
                )?;
                let range = edit
                    .map(|e| query::valid_range(&doc.text, e.get("range").unwrap_or(&e["replace"])))
                    .transpose()?;
                if range.as_ref().is_some_and(|r| {
                    r.start.line != query.position.line
                        || r.end.line != query.position.line
                        || r.start > query.position
                        || r.end < query.position
                }) {
                    return Err(LanguageError::ProtocolViolation);
                }
                let additional = item
                    .get("additionalTextEdits")
                    .map(|v| edits(&doc.text, v))
                    .transpose()?
                    .unwrap_or_default();
                if let Some(range) = &range {
                    if additional
                        .iter()
                        .any(|e| e.range.start <= range.end && e.range.end >= range.start)
                    {
                        return Err(LanguageError::ProtocolViolation);
                    }
                } else if !additional.is_empty() {
                    continue;
                }
                let sort_text = item
                    .get("sortText")
                    .map(|v| string(v, 4096))
                    .transpose()?
                    .unwrap_or_else(|| label.clone());
                let filter_text = item
                    .get("filterText")
                    .map(|v| string(v, 4096))
                    .transpose()?
                    .unwrap_or_else(|| label.clone());
                items.push(Completion {
                    label,
                    detail,
                    kind: item["kind"].as_u64().unwrap_or(1),
                    insert_text,
                    range,
                    additional,
                    snippet: item["insertTextFormat"] == 2,
                    sort_text,
                    filter_text,
                });
            }
            Ok(QueryResult::Completion(Completions {
                items,
                incomplete: value["isIncomplete"] == true || entries.len() > 256,
            }))
        }
        _ => Err(LanguageError::UnsupportedFeature),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn range(start: u32, end: u32) -> Value {
        json!({"start":{"line":0,"character":start},"end":{"line":0,"character":end}})
    }
    fn request(kind: QueryKind) -> LanguageQuery {
        LanguageQuery {
            request_id: uuid::Uuid::new_v4().to_string(),
            kind,
            path: "lib.rs".into(),
            version: 1,
            position: Position {
                line: 0,
                character: 5,
            },
            new_name: None,
        }
    }
    fn docs() -> BTreeMap<String, Arc<LanguageDocument>> {
        BTreeMap::from([(
            "lib.rs".into(),
            Arc::new(LanguageDocument {
                path: "lib.rs".into(),
                version: 1,
                text: "fn target() {} // 🦀\n".into(),
            }),
        )])
    }
    #[test]
    fn edits_reject_overlap_surrogate_splits_and_annotations() {
        let text = "🦀abc\n";
        for raw in [
            json!([{"range":range(1,2),"newText":"x"}]),
            json!([{"range":range(2,4),"newText":"x"},{"range":range(3,5),"newText":"y"}]),
            json!([{"range":range(2,2),"newText":"x"},{"range":range(2,2),"newText":"y"}]),
            json!([{"range":range(2,3),"newText":"x","annotationId":"confirm"}]),
        ] {
            assert!(edits(text, &raw).is_err());
        }
        assert_eq!(
            edits(text, &json!([{"range":range(2,5),"newText":"renamed"}]))
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn rename_rejects_external_paths_resource_operations_and_stale_versions() {
        let docs = docs();
        let root = Path::new("/tmp");
        let query = request(QueryKind::Rename);
        for raw in [
            json!({"changes":{"file:///etc/lib.rs":[{"range":range(3,9),"newText":"name"}]}}),
            json!({"documentChanges":[{"kind":"rename","oldUri":"file:///workspace/lib.rs","newUri":"file:///workspace/new.rs"}]}),
            json!({"documentChanges":[{"textDocument":{"uri":"file:///workspace/lib.rs","version":2},"edits":[]}]}),
        ] {
            assert!(parse(root, &query, raw, &docs).is_err());
        }
        let result = parse(
            root,
            &query,
            json!({"changes":{"file:///workspace/lib.rs":[{"range":range(3,9),"newText":"name"}]}}),
            &docs,
        )
        .unwrap();
        let QueryResult::Rename(changes) = result else {
            panic!()
        };
        assert_eq!(changes[0].original, docs["lib.rs"].text);
        assert_eq!(changes[0].edits[0].new_text, "name");
    }
    #[test]
    fn completion_keeps_snippets_but_never_returns_server_commands() {
        let result = parse(Path::new("/tmp"), &request(QueryKind::Completion), json!({"isIncomplete":true,"items":[
            {"label":"target", "kind":3,"insertTextFormat":2,"textEdit":{"range":range(3,9),"newText":"target(${1:value})"}},
            {"label":"danger", "command":{"command":"run"}}
        ]}), &docs()).unwrap();
        let QueryResult::Completion(result) = result else {
            panic!()
        };
        assert_eq!(result.items.len(), 1);
        assert!(result.items[0].snippet);
        assert!(result.incomplete);
        assert!(parse(Path::new("/tmp"), &request(QueryKind::Completion), json!([{"label":"bad","textEdit":{"range":range(3,9),"newText":"target"},"additionalTextEdits":[{"range":range(3,4),"newText":"overlap"}]}]), &docs()).is_err());
    }
    #[test]
    fn formatting_is_bounded_and_null_means_no_change() {
        let query = request(QueryKind::Formatting);
        assert!(
            matches!(parse(Path::new("/tmp"), &query, Value::Null, &docs()).unwrap(), QueryResult::Formatting(v) if v.is_empty())
        );
        assert!(
            parse(
                Path::new("/tmp"),
                &query,
                json!([{"range":range(0,0),"newText":"x".repeat(MAX_DOCUMENT)}]),
                &docs()
            )
            .is_err()
        );
    }
}
