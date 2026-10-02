// Navigable compiler diagnostics from the latest Cargo task. Paths are validated by
// the backend; this view only renders inert text and opens workspace locations.
export function createCargoProblems({ section, title, list, navigate, onCount }) {
  let items = [];

  function render(diagnostics, label) {
    items = Array.isArray(diagnostics?.items) ? diagnostics.items : [];
    list.replaceChildren();
    section.hidden = !items.length;
    title.textContent = `Cargo · ${label}${diagnostics?.truncated ? " · resultados limitados" : ""}`;
    for (const item of items) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "problem-item";
      button.dataset.severity = item.severity === "error" ? "1" : "2";
      button.dataset.cargoPath = item.path;
      const icon = document.createElement("span");
      icon.textContent = item.severity === "error" ? "●" : "▲";
      const location = document.createElement("span");
      location.className = "problem-location";
      location.textContent = `${item.path}:${item.line}:${item.column}`;
      const text = document.createElement("span");
      text.textContent = item.code ? `${item.message} [${item.code}]` : item.message;
      button.append(icon, location, text);
      button.addEventListener("click", () => navigate(item.path, { line: item.line - 1, character: item.column - 1 }));
      list.append(button);
    }
    onCount(items.length);
  }

  return {
    show: render,
    clear: () => render(null, ""),
    count: () => items.length,
  };
}
