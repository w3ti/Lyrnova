import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

// This module owns one ephemeral PTY for the current workspace. It never
// persists terminal input/output or exposes the emulator on window.
export function createTerminalView(element, invoke, listen) {
  const term = new Terminal({
    cols: 80, rows: 12, scrollback: 5000, fontSize: 11,
    fontFamily: 'ui-monospace, "SFMono-Regular", Consolas, monospace',
    cursorBlink: true, screenReaderMode: true, disableStdin: true,
    windowOptions: {},
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.open(element);
  // Terminal output must not access the clipboard or open arbitrary links.
  term.parser.registerOscHandler(52, () => true);
  term.parser.registerOscHandler(8, () => true);
  let sessionId = null;
  let generation = 0;
  let ended = false;
  let starting = null;
  let inputQueue = [];
  let queuedBytes = 0;
  let writing = false;
  let ackSequence = 0;
  let acknowledging = false;
  let resizeTimer = null;
  let resetBarrier = Promise.resolve();
  const encoder = new TextEncoder();
  const pause = () => new Promise(resolve => setTimeout(resolve, 20));

  function notice(message) { term.writeln(`\r\n[${message}]`); }

  async function acknowledge() {
    if (acknowledging || !sessionId) return;
    acknowledging = true;
    const id = sessionId;
    try {
      while (ackSequence && id === sessionId) {
        const sequence = ackSequence;
        try {
          await invoke("terminal_ack", { sessionId: id, sequence });
          if (id === sessionId && ackSequence === sequence) ackSequence = 0;
        } catch (error) {
          if (error?.code !== "busy") break;
          await pause();
        }
      }
    } finally {
      acknowledging = false;
      if (sessionId && id !== sessionId && ackSequence) void acknowledge();
    }
  }

  const listening = listen ? listen("terminal-event", ({ payload }) => {
    if (payload.sessionId !== sessionId) return;
    if (payload.kind === "output") {
      const id = sessionId;
      term.write(new Uint8Array(payload.data), () => {
        if (id !== sessionId) return;
        ackSequence = Math.max(ackSequence, payload.sequence);
        void acknowledge();
      });
    } else if (payload.kind === "exit") {
      sessionId = null;
      element.dataset.sessionId = "";
      element.dataset.state = "exited";
      ended = true;
      term.options.disableStdin = true;
      inputQueue = [];
      queuedBytes = 0;
      notice(`Sessão encerrada${payload.exitCode !== null ? ` · código ${payload.exitCode}` : ""}. Use + para iniciar outra.`);
    }
  }).then(() => true).catch(() => {
    notice("Streaming do terminal indisponível");
    return false;
  }) : Promise.resolve(false);

  async function sendInput() {
    if (writing || !sessionId) return;
    writing = true;
    const id = sessionId;
    try {
      while (inputQueue.length && id === sessionId) {
        const data = inputQueue[0];
        try {
          await invoke("terminal_write", { sessionId: id, input: Array.from(data) });
          if (id !== sessionId) break;
          inputQueue.shift();
          queuedBytes -= data.length;
        } catch (error) {
          if (id !== sessionId) break;
          if (error?.code === "busy") { await pause(); continue; }
          inputQueue = [];
          queuedBytes = 0;
          notice("Não foi possível enviar a entrada ao terminal");
          break;
        }
      }
    } finally {
      writing = false;
      if (sessionId && id !== sessionId && inputQueue.length) void sendInput();
    }
  }

  function queueInput(data) {
    if (!sessionId || !data.length) return;
    if (queuedBytes + data.length > 128 * 1024) {
      notice("Entrada grande demais; aguarde e cole um trecho menor");
      return;
    }
    for (let offset = 0; offset < data.length; offset += 8192) inputQueue.push(data.slice(offset, offset + 8192));
    queuedBytes += data.length;
    void sendInput();
  }
  term.onData(data => queueInput(encoder.encode(data)));
  term.onBinary(data => queueInput(Uint8Array.from(data, character => character.charCodeAt(0))));
  term.attachCustomKeyEventHandler(event => {
    // Keep copy distinct from Ctrl+C (interrupt). Let the browser handle
    // Ctrl+Shift+C/V and let the IDE handle its terminal toggle shortcut.
    if (event.ctrlKey && (event.key === "`" || (event.shiftKey && ["c", "v"].includes(event.key.toLowerCase())))) return false;
    return true;
  });

  function fit() {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(async () => {
      if (!element.clientWidth || !element.clientHeight || element.hidden) return;
      const size = fitAddon.proposeDimensions();
      if (!size) return;
      const cols = Math.min(500, Math.max(2, size.cols));
      const rows = Math.min(500, Math.max(1, size.rows));
      if (term.cols !== cols || term.rows !== rows) term.resize(cols, rows);
      const id = sessionId;
      if (id) {
        try { await invoke("terminal_resize", { sessionId: id, cols, rows }); }
        catch (error) { if (id === sessionId && error?.code === "busy") fit(); }
      }
    }, 50);
  }
  new ResizeObserver(fit).observe(element);

  function reset() {
    ++generation;
    sessionId = null;
    ended = false;
    inputQueue = [];
    queuedBytes = 0;
    ackSequence = 0;
    element.dataset.sessionId = "";
    element.dataset.state = "idle";
    term.options.disableStdin = true;
    // Queue the reset after already-buffered writes so bytes from an old
    // session cannot be parsed into the new terminal after the reset.
    resetBarrier = new Promise(resolve => term.write("", () => { term.reset(); resolve(); }));
  }

  async function start(restart = false) {
    if (!invoke || !(await listening)) return;
    while (starting) await starting;
    if (!restart && (sessionId || ended)) { fit(); return; }
    const previousSession = sessionId;
    if (restart) reset();
    const token = generation;
    starting = (async () => {
      try {
        await resetBarrier;
        if (token !== generation) return;
        if (restart && previousSession) await invoke("terminal_stop", { sessionId: previousSession });
        if (token !== generation) return;
        const session = await invoke("terminal_start", { cols: term.cols, rows: term.rows });
        if (token !== generation) return;
        sessionId = session.sessionId;
        element.dataset.sessionId = sessionId;
        element.dataset.state = "running";
        term.options.disableStdin = false;
        // No output is sent until both the event listener and session ID exist.
        await invoke("terminal_ack", { sessionId, sequence: 0 });
        fit();
      } catch (error) {
        if (token !== generation) return;
        sessionId = null;
        element.dataset.state = "error";
        term.options.disableStdin = true;
        notice(error?.code === "unsupported" ? "Terminal PTY disponível apenas no Linux" : "Não foi possível iniciar o terminal");
      }
    })();
    try { await starting; } finally { starting = null; }
  }

  return {
    start, reset, fit,
    focus: () => term.focus(),
    configure(fontSize, dark, reduceMotion) {
      term.options.fontSize = fontSize;
      term.options.cursorBlink = !reduceMotion;
      term.options.theme = dark
        ? { background: "#090d22", foreground: "#d4dcf1", cursor: "#c4a5ff", selectionBackground: "#4b3e6e" }
        : { background: "#f5f6fa", foreground: "#25304c", cursor: "#6847a5", selectionBackground: "#ddd1f4" };
      fit();
    },
  };
}
