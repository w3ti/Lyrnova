// Only typed, validated LSP results reach these providers; commands and markdown
// from the server never become executable editor actions.
export function registerRustActions({ monaco, query, rangeFor, prepareRename, navigate, sourceViewer, report }) {
  const editFor = edit => ({ range: rangeFor(edit.range), text: edit.newText });
  const codeActions = new WeakMap();
  const kinds = [18, 18, 0, 1, 2, 3, 4, 5, 7, 8, 9, 12, 13, 15, 17, 28, 19, 20, 21, 23, 16, 14, 6, 10, 11, 24];
  monaco.languages.registerCompletionItemProvider("rust", {
    triggerCharacters: [".", ":"],
    async provideCompletionItems(model, position, _context, token) {
      const result = await query("completion", model, position, token);
      if (result?.kind !== "completion" || !result.isCurrent()) return null;
      // Monaco can accept before lazy resolution finishes. Resolve imports before
      // offering the item so Enter always inserts the symbol and use together.
      const candidates = [];
      let resolving = 0, incomplete = result.value.incomplete;
      for (const item of result.value.items) {
        if (item.resolveId && resolving++ >= 32) { incomplete = true; continue; }
        candidates.push(item);
      }
      let next = 0;
      const ready = new Array(candidates.length);
      await Promise.all(Array.from({ length: Math.min(4, candidates.length) }, async () => {
        while (next < candidates.length && result.isCurrent() && !token.isCancellationRequested) {
          const index = next++, item = candidates[index];
          if (!item.resolveId) { ready[index] = item; continue; }
          const resolved = await query("completion_resolve", model, position, token, { resolveId: item.resolveId });
          if (resolved?.kind === "completion" && resolved.isCurrent()) ready[index] = resolved.value.items[0];
          else incomplete = true;
        }
      }));
      if (!result.isCurrent() || token.isCancellationRequested) return null;
      const word = model.getWordUntilPosition(position);
      const fallback = { startLineNumber: position.lineNumber, endLineNumber: position.lineNumber, startColumn: word.startColumn, endColumn: word.endColumn };
      return { incomplete, suggestions: ready.filter(Boolean).map(item => {
        const suggestion = {
          label: item.label, detail: item.detail, kind: kinds[item.kind] ?? 18,
          insertText: item.insertText, insertTextRules: item.snippet ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet : 0,
          range: item.range ? rangeFor(item.range) : fallback, additionalTextEdits: item.additional.map(editFor),
          sortText: item.sortText, filterText: item.filterText,
        };
        return suggestion;
      }) };
    },
  });
  monaco.languages.registerCodeActionProvider("rust", {
    async provideCodeActions(model, range, context, token) {
      if (context.only && context.only !== "quickfix" && !context.only.startsWith("quickfix.")) return null;
      const position = { lineNumber: range.startLineNumber, column: range.startColumn };
      const result = await query("code_action", model, position, token, { end: { line: range.endLineNumber - 1, character: range.endColumn - 1 } });
      if (result?.kind !== "code_action" || !result.isCurrent()) return null;
      return { actions: result.value.map(item => {
        const action = { title: item.title, kind: "quickfix", isPreferred: item.preferred };
        codeActions.set(action, { item, isCurrent: result.isCurrent });
        return action;
      }), dispose() {} };
    },
    async resolveCodeAction(action, token) {
      report("");
      const entry = codeActions.get(action);
      try {
        if (!entry || !entry.isCurrent() || token.isCancellationRequested) throw new Error("stale action");
        action.edit = { edits: await prepareRename(entry.item.edits, () => entry.isCurrent() && !token.isCancellationRequested) };
      } catch {
        action.edit = { edits: [] };
        report("Não foi possível aplicar a correção. O código pode ter mudado; consulte as correções novamente.");
      }
      return action;
    },
  }, { providedCodeActionKinds: ["quickfix"] });
  monaco.languages.registerDocumentFormattingEditProvider("rust", {
    displayName: "rustfmt",
    async provideDocumentFormattingEdits(model, _options, token) {
      report("");
      const result = await query("formatting", model, { lineNumber: 1, column: 1 }, token);
      if (result?.kind !== "formatting" || !result.isCurrent()) { report("Não foi possível formatar. Verifique se rustfmt está instalado na toolchain selecionada e se o código é válido."); return null; }
      return result.value.map(editFor);
    },
  });
  monaco.languages.registerRenameProvider("rust", {
    async provideRenameEdits(model, position, newName, token) {
      const result = await query("rename", model, position, token, { newName });
      const reject = () => ({ edits: [], rejectReason: "Não foi possível renomear. Verifique o nome e tente novamente; os arquivos podem ter mudado ou estar fora do workspace." });
      if (result?.kind !== "rename" || !result.isCurrent() || !result.value.length) return reject();
      try {
        return { edits: await prepareRename(result.value, result.isCurrent) };
      } catch { return reject(); }
    },
  });
  const dialog = document.querySelector("#rust-references-dialog");
  const list = document.querySelector("#rust-references-list");
  const status = document.querySelector("#rust-references-status");
  let epoch = 0;
  let cancellation = null;
  function reset() { epoch++; cancellation?.cancel(); cancellation = null; if (dialog.open) dialog.close(); list.replaceChildren(); }
  dialog.addEventListener("keydown", event => event.stopPropagation());
  dialog.addEventListener("close", () => { epoch++; cancellation?.cancel(); cancellation = null; });
  document.querySelector("#rust-references-close").addEventListener("click", reset);
  async function references(editor) {
    reset();
    const model = editor.getModel();
    if (!model || model.getLanguageId() !== "rust") return;
    const current = epoch;
    const listeners = new Set();
    const token = { isCancellationRequested: false, onCancellationRequested(fn) { listeners.add(fn); return { dispose: () => listeners.delete(fn) }; }, cancel() { this.isCancellationRequested = true; for (const fn of listeners) fn(); } };
    cancellation = token;
    status.textContent = "Buscando referências…"; dialog.showModal();
    const result = await query("references", model, editor.getPosition(), token);
    if (epoch !== current) return;
    if (result?.kind !== "references" || !result.isCurrent()) { status.textContent = "Não foi possível consultar. O código pode ter mudado; tente novamente."; return; }
    status.textContent = `${result.value.length} referência(s), incluindo a declaração.`;
    for (const item of result.value) {
      const button = document.createElement("button"); button.type = "button"; button.className = "rust-reference-item";
      button.textContent = `${item.source?.label ?? item.path}:${item.range.start.line + 1}:${item.range.start.character + 1}`;
      button.addEventListener("click", async () => {
        if (!result.isCurrent()) { list.replaceChildren(); status.textContent = "O código mudou. Consulte as referências novamente."; return; }
        // Validate before closing, which cancels the token.
        reset();
        if (item.source) sourceViewer.show(item.source, item.range.start);
        else await navigate(item.path, item.range.start);
      });
      list.append(button);
    }
  }
  return { references, reset };
}
