export function createRustEnvironment({ invoke, getWorkspace, isEnabled, onApplied }) {
  const button = document.querySelector("#language-environment");
  const dialog = document.querySelector("#language-environment-dialog");
  const select = document.querySelector("#language-toolchain");
  const detail = document.querySelector("#language-toolchain-detail");
  const dependencies = document.querySelector("#language-dependencies");
  const registry = document.querySelector("#language-registry");
  const paths = document.querySelector("#language-registry-paths");
  const error = document.querySelector("#language-environment-error");
  const apply = document.querySelector("#language-environment-apply");
  let epoch = 0, review = null, workspace = null, applying = false;
  dialog.addEventListener("keydown", event => event.stopPropagation());
  function selection() {
    const tool = review?.toolchains.find(t => t.id === select.value);
    detail.textContent = tool ? `${tool.path}\n${tool.standardLibrary ? `Biblioteca padrão: ${tool.standardLibrary}` : "Fontes da biblioteca padrão não instaladas (rust-src)."}` : "Nenhuma toolchain instalada foi encontrada.";
    registry.disabled = !dependencies.checked || !review?.registryPaths.length;
    if (registry.disabled) registry.checked = false;
    apply.disabled = applying || !tool;
  }
  function reset() { epoch++; review = null; workspace = null; dialog.close(); }
  button.addEventListener("click", async () => {
    if (!invoke || !isEnabled() || !getWorkspace()) return;
    const current = ++epoch;
    workspace = getWorkspace(); review = null; error.textContent = "";
    select.replaceChildren(); detail.textContent = "Buscando ferramentas instaladas…";
    paths.textContent = ""; dependencies.checked = true; registry.checked = false; apply.disabled = true;
    dialog.showModal();
    try {
      const result = await invoke("language_environment_review", { workspace });
      if (current !== epoch || workspace !== getWorkspace() || !isEnabled()) return;
      review = result;
      for (const tool of result.toolchains) {
        const option = document.createElement("option"); option.value = tool.id; option.textContent = tool.label; select.append(option);
      }
      paths.textContent = result.registryPaths.length ? result.registryPaths.join("\n") : "Nenhum cache de crates disponível.";
      selection();
    } catch { if (current === epoch) error.textContent = "Não foi possível revisar o ambiente Rust. Feche e tente novamente."; }
  });
  select.addEventListener("change", selection);
  dependencies.addEventListener("change", selection);
  document.querySelector("#language-environment-cancel").addEventListener("click", () => { if (!applying) reset(); });
  dialog.addEventListener("cancel", event => { if (applying) event.preventDefault(); else reset(); });
  apply.addEventListener("click", async () => {
    if (!review || applying || workspace !== getWorkspace() || !isEnabled()) return;
    const current = epoch;
    applying = true; apply.disabled = true;
    try {
      await invoke("language_environment_configure", { workspace, choice: {
        token: review.token, toolchainId: select.value, dependencies: dependencies.checked, useRegistry: registry.checked,
      } });
      if (current !== epoch || workspace !== getWorkspace() || !isEnabled()) return;
      dialog.close(); onApplied();
    } catch {
      if (current === epoch) { review = null; error.textContent = "O ambiente mudou ou a revisão expirou. Feche e abra a revisão novamente."; }
    } finally { applying = false; }
  });
  return { reset, configure: enabled => { button.hidden = !enabled; } };
}

export function createRustSourceViewer(monaco) {
  const dialog = document.querySelector("#language-source-dialog");
  const container = document.querySelector("#language-source-editor");
  const title = document.querySelector("#language-source-title");
  let editor = null, model = null;
  dialog.addEventListener("keydown", event => event.stopPropagation());
  function dispose() { editor?.dispose(); model?.dispose(); editor = null; model = null; }
  function reset() { dialog.close(); dispose(); }
  document.querySelector("#language-source-close").addEventListener("click", reset);
  dialog.addEventListener("close", dispose);
  return { reset, show(source, position) {
    dispose(); title.textContent = source.label;
    model = monaco.editor.createModel(source.content, "rust");
    dialog.showModal();
    editor = monaco.editor.create(container, { model, readOnly: true, domReadOnly: true, fontSize: 16,
      automaticLayout: true, minimap: { enabled: false }, scrollBeyondLastLine: false });
    editor.setPosition({ lineNumber: position.line + 1, column: position.character + 1 });
    editor.revealPositionInCenter(editor.getPosition()); editor.focus();
  } };
}
