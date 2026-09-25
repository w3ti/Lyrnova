import test from "node:test";
import assert from "node:assert/strict";
import { createRustEnvironment, createRustSourceViewer } from "../rust-environment.js";

function dom() {
  const nodes = new Map();
  const element = () => ({ value: "", checked: false, disabled: false, hidden: false, open: false, textContent: "", listeners: {}, children: [],
    addEventListener(name, fn) { this.listeners[name] = fn; },
    replaceChildren() { this.children = []; this.value = ""; },
    append(child) { this.children.push(child); if (!this.value) this.value = child.value; },
    showModal() { this.open = true; }, close() { this.open = false; this.listeners.close?.(); },
  });
  globalThis.document = { querySelector(id) { if (!nodes.has(id)) nodes.set(id, element()); return nodes.get(id); }, createElement: element };
  const get = id => document.querySelector(`#${id}`);
  return { get, emit: async (id, event) => get(id).listeners[event]?.({ preventDefault() {} }) };
}
const review = { token: "review-token", toolchains: [{ id: "rustup:stable", label: "stable", path: "/tools/stable", standardLibrary: "/tools/stable/library" }], registryPaths: ["/cargo/registry/src"] };

test("review opens without granting cache access; explicit Apply sends only selected options", async () => {
  const { get, emit } = dom();
  const calls = []; let applied = false;
  const view = createRustEnvironment({ invoke: async (command, args) => { calls.push({ command, args }); return review; }, getWorkspace: () => "/project", isEnabled: () => true, onApplied: () => { applied = true; } });
  view.configure(true);
  await emit("language-environment", "click");
  assert.equal(get("language-registry").checked, false);
  assert.equal(calls.length, 1);
  assert.equal(get("language-registry-paths").textContent, "/cargo/registry/src");
  get("language-registry").checked = true;
  await emit("language-environment-apply", "click");
  assert.deepEqual(calls[1], { command: "language_environment_configure", args: { workspace: "/project", choice: { token: "review-token", toolchainId: "rustup:stable", dependencies: true, useRegistry: true } } });
  assert.equal(applied, true);
});

test("cancel and reset cannot apply a review or revive an old workspace", async () => {
  const { get, emit } = dom(); let resolve, applied = false;
  const view = createRustEnvironment({ invoke: () => new Promise(r => { resolve = r; }), getWorkspace: () => "/project", isEnabled: () => true, onApplied: () => { applied = true; } });
  const pending = emit("language-environment", "click");
  view.reset(); resolve(review); await pending;
  assert.equal(get("language-environment-dialog").open, false);
  assert.equal(get("language-toolchain").children.length, 0);
  await emit("language-environment-apply", "click");
  assert.equal(applied, false);
});

test("disabling dependency analysis also clears cache consent", async () => {
  const { get, emit } = dom();
  createRustEnvironment({ invoke: async () => review, getWorkspace: () => "/project", isEnabled: () => true, onApplied() {} });
  await emit("language-environment", "click");
  get("language-registry").checked = true; get("language-dependencies").checked = false;
  await emit("language-dependencies", "change");
  assert.equal(get("language-registry").checked, false);
  assert.equal(get("language-registry").disabled, true);
});

test("external source viewer uses a read-only model and disposes it on revocation", () => {
  const { get } = dom(); let options, disposed = 0;
  const monaco = { editor: { createModel: () => ({ dispose: () => disposed++ }), create: (_node, config) => {
    options = config; return { setPosition() {}, getPosition() { return {}; }, revealPositionInCenter() {}, focus() {}, dispose: () => disposed++ };
  } } };
  const viewer = createRustSourceViewer(monaco);
  viewer.show({ label: "<script>literal</script>", content: "fn dependency() {}" }, { line: 0, character: 3 });
  assert.equal(options.readOnly, true); assert.equal(options.domReadOnly, true);
  assert.equal(get("language-source-title").textContent, "<script>literal</script>");
  viewer.reset();
  assert.equal(get("language-source-dialog").open, false); assert.equal(disposed, 2);
});
