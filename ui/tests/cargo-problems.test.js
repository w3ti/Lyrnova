import test from "node:test";
import assert from "node:assert/strict";
import { createCargoProblems } from "../cargo-problems.js";

function element() {
  return { hidden: false, textContent: "", dataset: {}, children: [], listeners: {},
    addEventListener(name, fn) { this.listeners[name] = fn; },
    replaceChildren() { this.children = []; },
    append(...children) { this.children.push(...children); } };
}

test("Cargo diagnostics render as inert text and navigate to zero-based positions", () => {
  globalThis.document = { createElement: element };
  const section = element(), title = element(), list = element();
  const visits = [], counts = [];
  const view = createCargoProblems({ section, title, list, navigate: (path, position) => visits.push({ path, position }), onCount: (n) => counts.push(n) });
  view.show({ items: [
    { path: "src/lib.rs", line: 3, column: 5, severity: "error", message: "<img src=x> cannot find value", code: "E0425" },
    { path: "src/main.rs", line: 1, column: 1, severity: "warning", message: "unused", code: null },
  ], truncated: true }, "cargo build");
  assert.equal(section.hidden, false);
  assert.equal(title.textContent, "Cargo · cargo build · resultados limitados");
  assert.equal(list.children.length, 2);
  const [error, warning] = list.children;
  assert.equal(error.dataset.severity, "1");
  assert.equal(warning.dataset.severity, "2");
  assert.equal(error.children[1].textContent, "src/lib.rs:3:5");
  assert.equal(error.children[2].textContent, "<img src=x> cannot find value [E0425]");
  error.listeners.click();
  assert.deepEqual(visits, [{ path: "src/lib.rs", position: { line: 2, character: 4 } }]);
  assert.equal(view.count(), 2);
  view.clear();
  assert.equal(section.hidden, true);
  assert.equal(list.children.length, 0);
  assert.deepEqual(counts, [2, 0]);
});
