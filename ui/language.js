const RUST_PLUGIN = "io.github.w3ti.lyrnova.language.rust";
const OWNER = "lyrnova-rust";
const messages = {
  permission_denied: "Revise as permissões do plugin Rust para iniciar a análise.",
  server_unavailable: "rust-analyzer não encontrado. Instale um binário independente no PATH do IDE.",
  sandbox_unavailable: "Não foi possível iniciar a análise isolada. Verifique o Bubblewrap e o rust-analyzer.",
  spawn_failed: "Não foi possível iniciar o rust-analyzer.",
  protocol_violation: "O servidor enviou uma resposta incompatível. Reinicie a análise.",
  timeout: "O servidor deixou de responder. Reinicie a análise.",
  server_exited: "O servidor encerrou. Reinicie a análise.",
  too_large: "A análise excedeu seus limites de tamanho.",
  invalid_document: "Um arquivo não pôde ser enviado à análise.",
};

export function createRustDiagnostics({ invoke, monaco, getWorkspace, getDocuments, modelFor, navigate, review }) {
  const status = document.querySelector("#language-status");
  const list = document.querySelector("#problems-list");
  const count = document.querySelector("#problems-count");
  const enable = document.querySelector("#language-enable");
  const restart = document.querySelector("#language-restart");
  let enabled = false;
  let generation = 0;
  let session = null;
  let timer = null;
  let busy = false;
  let failed = false;
  let restarting = false;
  let version = 0;
  let cache = new Map();
  let published = [];
  let lastSent = "";
  let excluded = 0;

  function setStatus(text, state = "idle") { status.textContent = text; status.dataset.state = state; }
  function clearMarkers() {
    for (const path of cache.keys()) {
      const model = modelFor(path);
      if (model && !model.isDisposed()) monaco.editor.setModelMarkers(model, OWNER, []);
    }
    published = []; render();
  }
  function render() {
    list.replaceChildren();
    let total = 0;
    for (const diagnostic of published) {
      const current = cache.get(diagnostic.path);
      if (!current || diagnostic.version !== current.version) continue;
      const model = modelFor(diagnostic.path);
      if (model && !model.isDisposed()) monaco.editor.setModelMarkers(model, OWNER, diagnostic.items.map(item => ({
        startLineNumber: item.range.start.line + 1, startColumn: item.range.start.character + 1,
        endLineNumber: item.range.end.line + 1, endColumn: item.range.end.character + 1,
        severity: [0, monaco.MarkerSeverity.Error, monaco.MarkerSeverity.Warning, monaco.MarkerSeverity.Info, monaco.MarkerSeverity.Hint][item.severity],
        message: item.message, source: "rust-analyzer", code: item.code ?? undefined,
      })));
      for (const item of diagnostic.items) {
        total++;
        const button = document.createElement("button");
        button.type = "button"; button.className = "problem-item"; button.dataset.severity = String(item.severity);
        button.dataset.problemPath = diagnostic.path;
        const icon = document.createElement("span"); icon.textContent = item.severity === 1 ? "●" : "▲";
        const location = document.createElement("span"); location.className = "problem-location";
        location.textContent = `${diagnostic.path}:${item.range.start.line + 1}:${item.range.start.character + 1}`;
        const text = document.createElement("span"); text.textContent = item.message;
        button.append(icon, location, text);
        button.addEventListener("click", () => {
          if (cache.get(diagnostic.path)?.version === diagnostic.version) navigate(diagnostic.path, item.range.start);
        });
        list.append(button);
      }
      if (diagnostic.truncated) {
        const note = document.createElement("p"); note.textContent = `${diagnostic.path}: resultados limitados.`; list.append(note);
      }
    }
    count.textContent = String(total);
  }
  function changed() {
    const next = new Map();
    let bytes = 0; excluded = 0;
    for (const doc of getDocuments().filter(d => d.path.endsWith(".rs"))) {
      const size = new TextEncoder().encode(doc.text).length;
      if (size > 512 * 1024 || next.size >= 32 || bytes + size > 8 * 1024 * 1024) { excluded++; continue; }
      bytes += size;
      const old = cache.get(doc.path);
      if (old?.text === doc.text) next.set(doc.path, old);
      else {
        next.set(doc.path, { ...doc, version: ++version });
        const model = modelFor(doc.path);
        if (model && !model.isDisposed()) monaco.editor.setModelMarkers(model, OWNER, []);
      }
    }
    for (const path of cache.keys()) if (!next.has(path)) {
      const model = modelFor(path);
      if (model && !model.isDisposed()) monaco.editor.setModelMarkers(model, OWNER, []);
    }
    cache = next;
    published = published.filter(d => cache.get(d.path)?.version === d.version);
    render(); schedule();
  }
  function schedule() {
    if (!invoke || timer || busy || failed) return;
    timer = setTimeout(() => { timer = null; void tick(); }, 300);
  }
  async function tick() {
    if (busy || !invoke || !getWorkspace() || !enabled || failed) return;
    if (!cache.size && !session) { setStatus(excluded ? "Arquivos acima do limite de análise (512 KiB por arquivo; até 32 abas)." : "Abra um arquivo Rust para analisar."); return; }
    busy = true;
    const currentGeneration = generation;
    try {
      if (!session) {
        setStatus("Iniciando análise Rust…", "starting");
        const result = await invoke("language_start", { workspace: getWorkspace(), restart: restarting });
        if (currentGeneration !== generation) return;
        session = result.sessionId; status.dataset.sessionId = session; restarting = false;
      }
      const docs = [...cache.values()];
      const fingerprint = JSON.stringify(docs.map(d => [d.path, d.version]));
      if (lastSent !== fingerprint) {
        await invoke("language_sync", { sessionId: session, documents: docs });
        if (currentGeneration !== generation) return;
        lastSent = fingerprint;
      }
      const result = await invoke("language_status", { sessionId: session });
      if (currentGeneration !== generation || result.sessionId !== session) return;
      if (result.state === "error" || result.state === "stopped") throw result.error ?? { code: "server_exited" };
      published = result.diagnostics;
      render();
      setStatus(result.state === "starting" ? "Iniciando análise Rust…" : `Rust: análise ativa${excluded ? ` · ${excluded} arquivo(s) excedem os limites` : ""}`, result.state);
    } catch (error) {
      if (currentGeneration === generation) {
        failed = true; clearMarkers();
        setStatus(messages[error?.code] ?? "Análise indisponível. Reinicie para tentar novamente.", "error");
      }
    } finally {
      busy = false;
      if (!failed && enabled && getWorkspace()) schedule();
    }
  }
  function reset() {
    generation++; clearTimeout(timer); timer = null; delete status.dataset.sessionId;
    clearMarkers(); cache = new Map(); session = null; lastSent = ""; failed = false;
    setStatus(enabled ? "Abra um arquivo Rust para analisar." : "Ative o plugin Rust para analisar os arquivos.");
  }
  function configure(plugins) {
    const next = Boolean(plugins.find(p => p.id === RUST_PLUGIN)?.enabled);
    if (next !== enabled) { enabled = next; reset(); }
    enable.hidden = enabled; restart.hidden = !enabled;
    if (!enabled) setStatus("Ative o plugin Rust para analisar os arquivos.");
    changed();
  }
  enable.addEventListener("click", review);
  restart.addEventListener("click", () => { reset(); restarting = true; changed(); });
  return { changed, reset, configure };
}
