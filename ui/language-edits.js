export async function prepareRenameDrafts(documents, isCurrent, { getGeneration, isBlocked, read, draftDocuments, savedDocuments, documentRevisions, recoveryConflicts, openDocuments, ensureDocumentModel, onPrepared }) {
  const generation = getGeneration();
  const snapshots = new Map();
  // Read all missing files first. No model or draft changes until every input matches.
  await Promise.all(documents.map(async doc => {
    if (!draftDocuments.has(doc.path)) snapshots.set(doc.path, await read(doc.path));
  }));
  if (generation !== getGeneration() || isBlocked() || !isCurrent()) throw new Error("stale rename");
  if (new Set([...openDocuments, ...documents.map(doc => doc.path)]).size > 32) throw new Error("too many tabs");
  for (const doc of documents) {
    if (recoveryConflicts.has(doc.path) || (draftDocuments.get(doc.path) ?? snapshots.get(doc.path)?.content) !== doc.original) throw new Error("document changed");
  }
  const edits = [];
  for (const doc of documents) {
    if (!draftDocuments.has(doc.path)) {
      const snapshot = snapshots.get(doc.path);
      savedDocuments.set(doc.path, snapshot.content); draftDocuments.set(doc.path, snapshot.content); documentRevisions.set(doc.path, snapshot.revision);
    }
    if (!openDocuments.includes(doc.path)) openDocuments.push(doc.path);
    const model = ensureDocumentModel(doc.path);
    for (const edit of doc.edits) edits.push({ resource: model.uri, versionId: model.getVersionId(), textEdit: {
      range: { startLineNumber: edit.range.start.line + 1, startColumn: edit.range.start.character + 1, endLineNumber: edit.range.end.line + 1, endColumn: edit.range.end.character + 1 }, text: edit.newText,
    } });
  }
  onPrepared();
  return edits;
}
