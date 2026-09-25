import test from "node:test";
import assert from "node:assert/strict";
import { createRustDiagnostics } from "../language.js";

function setup() {
  const nodes = new Map();
  const node = () => ({ dataset: {}, textContent: "", hidden: false, close() {}, showModal() {}, replaceChildren() {}, append() {}, addEventListener() {} });
  globalThis.document = { querySelector(selector) { if (!nodes.has(selector)) nodes.set(selector, node()); return nodes.get(selector); }, createElement: node };
  let text = "fn target() {}", modelVersion = 1, reply = { kind: "hover", value: { text: "fn target()", range: null } };
  const providers = {}, calls = [], navigations = [];
  const model = { getVersionId: () => modelVersion, isDisposed: () => false };
  const monaco = {
    languages: { registerCompletionItemProvider() {}, registerDocumentFormattingEditProvider() {}, registerRenameProvider() {}, registerHoverProvider(_language, provider) { providers.hover = provider.provideHover; }, registerDefinitionProvider(_language, provider) { providers.definition = provider.provideDefinition; } },
    editor: { setModelMarkers() {}, registerEditorOpener(opener) { providers.open = opener.openCodeEditor; } },
    Uri: { from: ({ path }) => ({ toString: () => `file://${path}` }) },
  };
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === "language_start") return { sessionId: "session" };
    if (command === "language_status") return { sessionId: "session", state: "running", diagnostics: [] };
    if (command === "language_query") return typeof reply === "function" ? reply(args) : reply;
  };
  const controller = createRustDiagnostics({ invoke, monaco, getWorkspace: () => "/project", getDocuments: () => [{ path: "src/lib.rs", text }], modelFor: () => model, navigate: async (...args) => navigations.push(args), review() {} });
  const plugins = [{ id: "io.github.w3ti.lyrnova.language.rust", enabled: true }];
  controller.configure(plugins);
  return { providers, model, calls, navigations, controller,
    reply(value) { reply = value; }, edit() { text += " "; modelVersion++; controller.changed(); },
    cleanup() { controller.configure([]); controller.reset(); },
  };
}
function token() {
  const listeners = new Set();
  return { isCancellationRequested: false, onCancellationRequested(fn) { listeners.add(fn); return { dispose: () => listeners.delete(fn) }; },
    cancel() { this.isCancellationRequested = true; for (const fn of listeners) fn(); } };
}
const position = { lineNumber: 1, column: 5 };

test("hover synchronizes first and renders documentation as inert literal text", async () => {
  const f = setup();
  try {
    f.reply({ kind: "hover", value: { text: "[run](command:evil) ![image](https://example.invalid) <img> `code`", range: null } });
    const hover = await f.providers.hover(f.model, position, token());
    assert.equal(hover.contents[0].isTrusted, false);
    assert.equal(hover.contents[0].supportHtml, false);
    assert.ok(hover.contents[0].value.startsWith("    [run](command:evil)"));
    assert.ok(hover.contents[0].value.includes("<img>"));
    assert.ok(hover.contents[0].value.split("\n").every(line => line.startsWith("    ")));
    const names = f.calls.map(c => c.command);
    assert.ok(names.indexOf("language_sync") < names.indexOf("language_query"));
    assert.deepEqual(f.calls.find(c => c.command === "language_query").args.query.position, { line: 0, character: 4 });
  } finally { f.cleanup(); }
});

for (const reason of ["edit", "cancel", "reset", "disable", "disk"]) {
  test(`pending hover is discarded on ${reason}`, async () => {
    const f = setup();
    try {
      let resolve, started;
      const ready = new Promise(r => { started = r; });
      f.reply(() => new Promise(r => { resolve = r; started(); }));
      const cancellation = token();
      const pending = f.providers.hover(f.model, position, cancellation);
      await ready;
      if (reason === "edit") f.edit();
      if (reason === "disk") f.controller.workspaceChanged();
      if (reason === "cancel") cancellation.cancel();
      if (reason === "reset") f.controller.reset();
      if (reason === "disable") f.controller.configure([]);
      resolve({ kind: "hover", value: { text: "stale", range: null } });
      assert.equal(await pending, null);
      assert.ok(f.calls.some(c => c.command === "language_cancel"));
    } finally { f.cleanup(); }
  });
}

test("definition opens the authorized target and discards targets after edits", async () => {
  const f = setup();
  try {
    f.reply({ kind: "definition", value: [{ path: "src/helper.rs", range: { start: { line: 2, character: 3 }, end: { line: 2, character: 9 } } }] });
    const locations = await f.providers.definition(f.model, position, token());
    assert.equal(locations[0].range.startLineNumber, 3);
    assert.equal(await f.providers.open(null, locations[0].uri, locations[0].range), true);
    assert.deepEqual(f.navigations, [["src/helper.rs", { line: 2, character: 3 }]]);
    f.edit();
    assert.equal(await f.providers.open(null, locations[0].uri, locations[0].range), false);
    assert.equal(await f.providers.open(null, { toString: () => "file:///etc/passwd" }), false);
  } finally { f.cleanup(); }
});
