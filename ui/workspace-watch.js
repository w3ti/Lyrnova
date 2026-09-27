// One bounded poll at a time. A cursor is acknowledged only after its refresh was
// handled; skipped tabs remain pending even if no further disk event arrives.
export function createWorkspaceMonitor({ invoke, context, ready, paths, reconcile, refresh, invalidateRust, status, interval = 1500 }) {
  let active = false, epoch = 0, token = null, timer = null, flight = null, delay = interval;
  const pending = new Set();
  const valid = (expected, generation) => active && epoch === generation && ready() && context() === expected;
  function schedule(wait = delay) {
    if (!active || timer !== null) return;
    timer = setTimeout(() => { timer = null; void pollNow(); }, wait);
  }
  function pollNow() {
    if (flight) return flight;
    flight = poll().finally(() => { flight = null; schedule(); });
    return flight;
  }
  async function poll() {
    if (!active || !invoke || !ready()) return;
    const expected = context(), generation = epoch;
    const current = () => valid(expected, generation);
    try {
      const result = await invoke("workspace_poll", { workspace: expected.workspace, token, paths: paths() });
      if (!current()) return;
      if (result.rustChanged) invalidateRust();
      if (result.resync) for (const path of paths()) pending.add(path);
      else for (const path of result.changes) pending.add(path);
      if (result.resync || result.changes.length) await refresh(result);
      if (!current()) return;
      const opened = new Set(paths());
      for (const path of [...pending]) {
        if (!opened.has(path) || await reconcile(path, current, expected.workspace)) pending.delete(path);
        if (!current()) return;
      }
      token = result.token;
      delay = result.mode === "polling" ? Math.max(interval, 5000) : interval;
      status(result.mode === "polling" ? "Atualização por compatibilidade · pode levar até 5 segundos" : "Arquivos acompanhados por eventos", "watching");
    } catch (error) {
      if (current()) status(error?.code === "too_large" ? "Atualização automática limitada: projeto excede os limites de arquivos ou tamanho." : "Atualização automática indisponível; tentando novamente…", "error");
    }
  }
  function stop() { active = false; epoch++; clearTimeout(timer); timer = null; token = null; pending.clear(); status("", "idle"); }
  function start() { stop(); delay = interval; active = true; schedule(0); }
  function wake() { if (active) { clearTimeout(timer); timer = null; void pollNow(); } }
  return { start, stop, wake, pollNow };
}

// The only writer of a refreshed snapshot. All decisions use the current draft,
// after I/O completes, to avoid overwriting text typed while a read was pending.
export async function reconcileDocument(path, isCurrent, { read, exists, busy, revision, identity, draft, dirty, accept, conflict }) {
  if (!exists(path)) return true;
  if (busy(path)) return false;
  const before = { revision: revision(path), identity: identity(path) };
  let snapshot;
  try { snapshot = await read(path); } catch (error) {
    if (!isCurrent()) return false;
    if (!["not_a_file", "invalid_path", "symbolic_link", "path_escapes_workspace", "binary_file", "not_utf8", "document_too_large"].includes(error?.code)) throw error;
    snapshot = null;
  }
  if (!isCurrent()) return false;
  if (!exists(path)) return true;
  if (busy(path) || revision(path) !== before.revision || identity(path) !== before.identity) return false;
  if (!snapshot) { conflict(path); return true; }
  if (snapshot.revision === before.revision || snapshot.content === draft(path)) { accept(path, snapshot, false); return true; }
  if (dirty(path)) conflict(path);
  else accept(path, snapshot, true);
  return true;
}
