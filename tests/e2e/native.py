#!/usr/bin/env python3
"""Real Tauri/WebKitGTK smoke; only Python's standard library is required.

The workspace and XDG profile are disposable. No synthetic IPC, test-only app
commands, accounts or provider are used. See README.md for dependencies/scope.
"""
import argparse
import base64
import http.client
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import traceback
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[2]


def wait_for(predicate, label, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(f"Timed out: {label}")


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Browser:
    def __init__(self, url):
        self.url = url
        self.session = None
        # Never route localhost WebDriver traffic through a proxy.
        self.http = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(self, method, path, data=None):
        request = urllib.request.Request(
            self.url + path, method=method,
            data=None if data is None else json.dumps(data).encode(),
            headers={"Content-Type": "application/json"},
        )
        try:
            with self.http.open(request, timeout=30) as response:
                value = json.load(response).get("value")
        except urllib.error.HTTPError as error:
            raise RuntimeError(f"{method} {path}: {error.read().decode()}") from error
        return value

    def command(self, method, path, data=None):
        return self.request(method, f"/session/{self.session}{path}", data)

    def start(self, binary):
        response = self.request("POST", "/session", {
            "capabilities": {"alwaysMatch": {"tauri:options": {"application": str(binary)}}},
        })
        self.session = response["sessionId"]
        self.command("POST", "/timeouts", {"script": 20000, "implicit": 0})
        return response["capabilities"]

    def script(self, script, *args):
        return self.command("POST", "/execute/sync", {"script": script, "args": list(args)})

    def invoke(self, command, args=None):
        result = self.command("POST", "/execute/async", {
            "script": "const done = arguments[2]; window.__TAURI__.core.invoke(arguments[0], arguments[1]).then(value => done({value}), error => done({error}));",
            "args": [command, args or {}],
        })
        if "error" in result:
            raise AssertionError(f"{command}: {result['error']}")
        return result.get("value")

    def click(self, selector):
        # Wry/WebKit can reject native click/sendKeys (tauri issue #6541).
        # Drive DOM handlers explicitly; this suite does not certify OS input.
        self.script("""
            const node = document.querySelector(arguments[0]);
            if (!node || node.disabled || !node.getClientRects().length)
                throw new Error('Control unavailable: ' + arguments[0]);
            node.scrollIntoView({block: 'nearest'});
            node.click();
        """, selector)

    def type(self, selector, text, replace=False):
        self.script("""
            const [selector, text, replace] = arguments;
            const node = document.querySelector(selector);
            if (!node || node.disabled || !node.getClientRects().length)
                throw new Error('Input unavailable: ' + selector);
            node.focus();
            if (replace) {
                node.dispatchEvent(new KeyboardEvent('keydown', {
                    key: 'a', code: 'KeyA', keyCode: 65, which: 65,
                    ctrlKey: true, bubbles: true, cancelable: true
                }));
                node.select();
            }
            if (text) {
                if (!document.execCommand('insertText', false, text))
                    throw new Error('Text insertion failed');
            } else if (replace) {
                document.execCommand('delete');
            }
        """, selector, text, replace)

    def terminal(self, command):
        self.until('document.querySelector("#terminal-output").dataset.state === "running"', "PTY ready")
        # Exercise xterm's real paste handler (including bracketed paste).
        # execCommand input is intentionally ignored in screen-reader mode.
        self.script("""
            const node = document.querySelector('#terminal-output .xterm-helper-textarea');
            node.focus();
            const event = new Event('paste', {bubbles: true, cancelable: true});
            const text = arguments[0];
            Object.defineProperty(event, 'clipboardData', {value: {getData: () => text}});
            node.dispatchEvent(event);
        """, command)
        self.terminal_key("Enter", 13)

    def terminal_key(self, key, key_code, ctrl=False):
        self.script("""
            const node = document.querySelector('#terminal-output .xterm-helper-textarea');
            node.focus();
            for (const type of ['keydown', 'keyup']) node.dispatchEvent(new KeyboardEvent(type, {
                key: arguments[0], keyCode: arguments[1], which: arguments[1],
                ctrlKey: arguments[2], bubbles: true, cancelable: true
            }));
        """, key, key_code, ctrl)

    def until(self, expression, label):
        return wait_for(lambda: self.script(f"return ({expression});"), label)

    def close(self):
        if self.session:
            self.command("DELETE", "")
            self.session = None


def git(workspace, *args):
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1")
    return subprocess.check_output(["git", "-C", str(workspace), *args], env=env, text=True).strip()


def running(pid):
    try:
        # A zombie cannot execute commands; its parent/reaper owns reaping it.
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


def process_memory(driver_pid):
    """Snapshot RSS of the driver's current descendants (not a peak/PSS)."""
    processes = {}
    for entry in Path("/proc").glob("[0-9]*"):
        try:
            fields = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            processes[int(entry.name)] = (int(fields[1]), int(fields[21]))
        except (OSError, ValueError, IndexError):
            continue
    family = {driver_pid}
    while True:
        expanded = family | {pid for pid, (parent, _) in processes.items() if parent in family}
        if expanded == family:
            break
        family = expanded
    return {"processes": len(family), "rssKiB": sum(processes.get(pid, (0, 0))[1] for pid in family) * os.sysconf("SC_PAGE_SIZE") // 1024}


def exercise(browser, workspace, report, started):
    browser.until('document.querySelector("#project-name")?.textContent === "workspace"', "restored project")
    browser.until('document.querySelector("#source-editor .view-lines")?.textContent.includes("initial")', "initial document")
    report["startupSeconds"] = round(time.monotonic() - started, 3)
    assert browser.invoke("ai_provider_current") is None
    assert isinstance(browser.invoke("plugin:window|is_maximized"), bool)
    try:
        browser.invoke("plugin:event|emit", {"event": "e2e-forbidden-event", "payload": None})
    except AssertionError as error:
        assert "not allowed" in str(error), str(error)
    else:
        raise AssertionError("Frontend unexpectedly allowed to emit events")
    assert browser.script('return [...document.querySelectorAll(".ai-plugin-control")].every(node => node.hidden)')
    assert browser.script('return getComputedStyle(document.documentElement).fontSize') == "16px"
    assert browser.script('return getComputedStyle(document.querySelector("#source-editor .view-lines")).fontSize') == "16px"
    report["checks"].append("restored project, no AI, 16px defaults")
    report["checks"].append("window state query allowed, frontend event emission denied")

    content = "edited via native UI - search-marker-917\n"
    browser.type("#source-editor textarea.inputarea", content, replace=True)
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "dirty"', "dirty draft")
    assert (workspace / "README.md").read_text() == "initial\n"
    started = time.monotonic()
    browser.click('[data-action="save-document"]')
    wait_for(lambda: (workspace / "README.md").read_text() == content, "saved file on disk")
    report["saveSeconds"] = round(time.monotonic() - started, 3)
    browser.type("#file-filter", "search-marker-917")
    browser.until('document.querySelector(".file-tree").textContent.includes("search-marker-917")', "content search")
    browser.type("#file-filter", "", replace=True)
    report["checks"].append("Monaco editing, explicit save and content search")

    browser.click('[data-activity="git"]')
    browser.until('!!document.querySelector(\'[data-git-action="stage"][data-git-path="README.md"]\')', "Git change")
    browser.click('[data-git-diff-path="README.md"]')
    browser.until('document.querySelector("#git-diff-editor .view-lines")?.textContent.includes("+edited")', "real worktree diff")
    browser.click('[data-git-action="stage"][data-git-path="README.md"]')
    browser.until('document.querySelector("#git-staged-count").textContent === "1"', "staged file")
    before = git(workspace, "rev-parse", "HEAD")
    browser.type("#git-commit-message", "Native E2E reviewed commit")
    browser.click('[data-action="git-commit"]')
    browser.until('document.querySelector("#git-commit-review-dialog").open && !document.querySelector("#git-review-confirm").disabled', "commit review")
    assert git(workspace, "rev-parse", "HEAD") == before
    browser.click('#git-commit-review-dialog [data-action="cancel-git-review"]')
    assert git(workspace, "rev-parse", "HEAD") == before
    browser.click('[data-action="git-commit"]')
    browser.until('document.querySelector("#git-commit-review-dialog").open && !document.querySelector("#git-review-confirm").disabled', "second review")
    browser.click("#git-review-confirm")
    browser.until('!document.querySelector("#git-commit-review-dialog").open', "confirmed commit")
    assert git(workspace, "rev-parse", "HEAD") != before
    assert git(workspace, "show", "HEAD:README.md") == content.strip()
    assert git(workspace, "status", "--porcelain") == ""
    report["checks"].append("real diff, stage, canceled review, confirmed commit")

    browser.terminal("printf 'native-%s\\n' 'output'; printf 'native-%s\\n' 'error' >&2")
    browser.until('document.querySelector("#terminal-output").textContent.includes("native-output") && document.querySelector("#terminal-output").textContent.includes("native-error")', "stdout and stderr")
    # The counter proves restart does not replay a previous terminal command.
    browser.terminal("printf 'run\\n' >> executions; sleep 300 & echo $! > descendant.pid")
    wait_for(lambda: (workspace / "descendant.pid").exists(), "terminal child")
    pid = int((workspace / "descendant.pid").read_text())
    report["terminalChildPid"] = pid
    assert running(pid)
    browser.click('[data-action="new-terminal"]')
    wait_for(lambda: not running(pid), "terminal restart terminates descendants", timeout=5)
    report["checks"].append("terminal stdout/stderr and descendant cleanup on restart")

    browser.terminal("test -t 0 && test -t 1 && test -t 2 && printf 'PTY-%s\\n' 'OK'; printf '\\033[31mPTY-%s\\033[0m\\n' 'COLOR'; stty size > pty-size")
    browser.until('document.querySelector("#terminal-output").textContent.includes("PTY-OK") && document.querySelector("#terminal-output").textContent.includes("PTY-COLOR")', "PTY and ANSI output")
    browser.until('[...document.querySelectorAll("#terminal-output .xterm-fg-1")].some(node => node.textContent.includes("PTY-COLOR"))', "ANSI red rendered")
    browser.terminal("printf '\\033]8;;https://example.invalid\\aLINK-CHECK\\033]8;;\\a\\n'")
    browser.until('document.querySelector("#terminal-output .xterm-rows").textContent.includes("LINK-CHECK")', "OSC text rendered")
    assert not browser.script('return !!document.querySelector("#terminal-output a[href]")'), "Terminal output created an active link"
    wait_for(lambda: (workspace / "pty-size").exists(), "PTY dimensions")
    initial_size = (workspace / "pty-size").read_text()
    browser.script('document.querySelector("#terminal-output").style.width = "600px"')
    time.sleep(0.2)
    browser.terminal("stty size > pty-resized")
    wait_for(lambda: (workspace / "pty-resized").exists(), "resized PTY dimensions")
    assert (workspace / "pty-resized").read_text() != initial_size
    browser.script('document.querySelector("#terminal-output").style.width = ""')
    browser.terminal("sh -c 'echo $$ > interrupt.pid; exec sleep 300'")
    wait_for(lambda: (workspace / "interrupt.pid").exists(), "foreground command")
    interrupt_pid = int((workspace / "interrupt.pid").read_text())
    browser.terminal_key("c", 67, ctrl=True)
    wait_for(lambda: not running(interrupt_pid), "Ctrl+C interrupts foreground command")
    browser.terminal("printf 'AFTER-%s\\n' 'INTERRUPT'")
    browser.until('document.querySelector("#terminal-output").textContent.includes("AFTER-INTERRUPT")', "shell usable after interrupt")
    browser.terminal_key("d", 68, ctrl=True)
    browser.until('document.querySelector("#terminal-output").dataset.state === "exited"', "Ctrl+D exits shell")
    browser.click('[data-action="new-terminal"]')
    browser.until('document.querySelector("#terminal-output").dataset.state === "running"', "new PTY after EOF")
    report["checks"].append("interactive PTY, ANSI, resize, Ctrl+C, Ctrl+D and restart")

    old_session = browser.script('return document.querySelector("#terminal-output").dataset.sessionId')
    browser.terminal("sh -c 'echo $$ > flood.pid; exec yes PTY-OLD'")
    browser.until('document.querySelector("#terminal-output .xterm-rows").textContent.includes("PTY-OLD")', "continuous PTY output")
    browser.click('[data-action="new-terminal"]')
    browser.until('document.querySelector("#terminal-output").dataset.state === "running"', "restart under output load")
    browser.terminal("printf 'PTY-%s\\n' 'FRESH'")
    browser.until('document.querySelector("#terminal-output .xterm-rows").textContent.includes("PTY-FRESH")', "new session after output load")
    assert not browser.script('return document.querySelector("#terminal-output .xterm-rows").textContent.includes("PTY-OLD")'), "Old output crossed sessions"
    for command, payload in [("terminal_write", {"input": [120]}), ("terminal_stop", {})]:
        try:
            browser.invoke(command, {"sessionId": old_session, **payload})
        except AssertionError as error:
            assert "stale_session" in str(error), str(error)
        else:
            raise AssertionError(f"{command} accepted a stale session")
    report["checks"].append("PTY restart under output load, clean screen and stale session rejection")

    browser.click('[data-activity="settings"]')
    browser.click('[data-plugin-toggle="io.github.w3ti.lyrnova.tool.e2e"]')
    browser.until('document.querySelector(\'[data-plugin-toggle="io.github.w3ti.lyrnova.tool.e2e"]\')?.dataset.enable === "false"', "external plugin activation")
    browser.click('[data-activity="tasks"]')
    browser.until('!!document.querySelector(\'[data-task-id="heartbeat"]\')', "external task discovery")
    assert browser.invoke("task_list")["sandbox"]["isolatedNetwork"] == "strong"
    browser.click('[data-task-id="heartbeat"]')
    browser.until('document.querySelector("#task-review-dialog").open', "task review")
    assert not (workspace / "heartbeat").exists()
    browser.click("#task-review-confirm")
    wait_for(lambda: (workspace / "heartbeat").exists() and (workspace / "heartbeat").stat().st_size > 2, "task descendant output")
    browser.until('document.querySelector("#task-output").textContent.includes("task-child-started")', "task event streaming")
    browser.click("#cancel-task-button")
    browser.until('document.querySelector("#cancel-task-button").hidden', "task cancellation finished")
    size = (workspace / "heartbeat").stat().st_size
    time.sleep(0.6)
    assert (workspace / "heartbeat").stat().st_size == size, "Task descendant survived cancellation"
    report["checks"].append("external plugin activation, sandboxed task review/streaming/cancellation")
    (workspace / "crash-plugin").touch()
    browser.click('[data-action="refresh-tasks"]')
    browser.until('document.querySelector("#tasks-status").dataset.state === "error"', "plugin crash surfaced")
    report["checks"].append("plugin crash reported while editor remains available")

    browser.click('[data-activity="explorer"]')
    browser.click('[data-file="README.md"]')
    browser.type("#source-editor textarea.inputarea", "unsaved draft", replace=True)
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "dirty" && document.querySelector("#source-editor .view-lines").textContent.includes("unsaved")', "draft after plugin failure")
    (workspace / "README.md").write_text("external edit\n")
    browser.click('[data-action="save-document"]')
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "error"', "external modification conflict")
    assert (workspace / "README.md").read_text() == "external edit\n"
    browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("unsaved")', "draft preserved after conflict")
    report["checks"].append("external modification refuses overwrite and preserves draft")
    # Keep several tabs, including a deleted file with a draft and a clean file.
    (workspace / "missing.txt").write_text("original missing file\n")
    (workspace / "clean.txt").write_text("".join(f"row {i}\n" for i in range(160)) + "original clean file")
    browser.click('[data-action="refresh-files"]')
    browser.until('!!document.querySelector(\'[data-file="missing.txt"]\')', "new recovery fixtures")
    browser.click('[data-file="missing.txt"]')
    browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("missing")', "missing fixture opened")
    browser.type("#source-editor textarea.inputarea", "draft of deleted file", replace=True)
    (workspace / "missing.txt").unlink()
    browser.click('[data-file="clean.txt"]')
    browser.until('document.querySelector("#source-editor .monaco-editor").dataset.uri.endsWith("/clean.txt")', "clean file tab")
    browser.script('document.querySelector("#source-editor textarea.inputarea").dispatchEvent(new KeyboardEvent("keydown", {key: "End", code: "End", keyCode: 35, which: 35, ctrlKey: true, bubbles: true, cancelable: true}))')
    browser.until('document.querySelector("#cursor-position").textContent.startsWith("Ln 161,")', "scrolled clean file")
    browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("original")', "smooth scroll reached last line")
    browser.until('document.querySelector("#session-state").dataset.state === "saved"', "scroll snapshot flushed")
    browser.click('[data-editor-path="README.md"]')
    browser.type("#source-editor textarea.inputarea", "unsaved draft\ncursor restored", replace=True)
    browser.until('document.querySelector("#cursor-position").textContent === "Ln 2, Col 16"', "cursor before restart")
    browser.until('document.querySelector("#session-state").dataset.state === "saved"', "recovery snapshot flushed")
    saved = browser.invoke("editor_session_load", {"workspace": str(workspace)})["session"]
    assert [doc["path"] for doc in saved["documents"]] == ["README.md", "missing.txt", "clean.txt"]
    assert saved["active"] == "README.md"
    assert saved["documents"][0]["draft"] == "unsaved draft\ncursor restored"
    assert saved["documents"][2]["scrollTop"] > 0
    for command, args, expected in [
        ("editor_session_load", {"workspace": str(workspace.parent)}, "workspace_changed"),
        ("editor_session_save", {"workspace": str(workspace), "token": "stale-token", "session": saved}, "conflict"),
    ]:
        try:
            browser.invoke(command, args)
        except AssertionError as error:
            assert expected in str(error), str(error)
        else:
            raise AssertionError(f"Session IPC accepted {expected}")
    report["checks"].append("session IPC rejects another workspace and stale snapshot tokens")
    (workspace / "clean.txt").write_text("".join(f"row {i}\n" for i in range(160)) + "changed clean file")



def rust_server_pids(driver_pid):
    parents = {}
    servers = set()
    for entry in Path("/proc").glob("[0-9]*"):
        try:
            pid = int(entry.name)
            parents[pid] = int((entry / "stat").read_text().rsplit(")", 1)[1].split()[1])
            if (entry / "cmdline").read_bytes().split(b"\0")[0] == b"/server/rust-analyzer":
                servers.add(pid)
        except (OSError, ValueError, IndexError):
            continue
    family = {driver_pid}
    while True:
        expanded = family | {pid for pid, parent in parents.items() if parent in family}
        if expanded == family:
            return servers & family
        family = expanded


def exercise_rust_diagnostics(browser, workspace, report, driver_pid):
    valid = 'pub fn answer() -> i32 { 42 }\n'
    invalid = 'pub fn answer() { let _ = "🦀"; let value = ; }\n'
    (workspace / "Cargo.toml").write_text('[package]\nname = "native_lsp"\nversion = "0.1.0"\nedition = "2021"\n')
    (workspace / "rust-analyzer.toml").write_text('checkOnSave = true\n[cargo.buildScripts]\nenable = true\n')
    (workspace / "src").mkdir(exist_ok=True)
    (workspace / "src/lib.rs").write_text(valid)
    (workspace / "build.rs").write_text('fn main() { std::fs::write("build-script-executed", "BAD").unwrap(); }\n')
    try:
        browser.invoke("language_start", {"workspace": str(workspace), "restart": False})
    except AssertionError as error:
        assert "permission_denied" in str(error), str(error)
    else:
        raise AssertionError("Rust server started without reviewed permissions")
    browser.click('[data-activity="explorer"]')
    browser.click('[data-action="refresh-files"]')
    browser.until('!!document.querySelector(\'[data-file="src/lib.rs"]\')', "Rust file in tree")
    browser.click('[data-file="src/lib.rs"]')
    browser.until('document.querySelector("#source-editor .view-lines")?.textContent.includes("answer")', "Rust document opened")
    browser.click('[data-dock-view="problems"]')
    browser.click('#language-enable')
    browser.until('document.querySelector("#builtin-review-dialog").open', "Rust permission review")
    assert browser.script('return document.querySelector("#builtin-review-confirm").disabled')
    browser.click('#builtin-review-cancel')
    assert not next(p for p in browser.invoke("plugin_list") if p["id"] == "io.github.w3ti.lyrnova.language.rust")["enabled"]
    browser.click('#language-enable')
    browser.script('document.querySelectorAll("#builtin-permissions input").forEach(input => { if (!input.checked) input.click(); })')
    browser.click('#builtin-review-confirm')
    browser.until('!document.querySelector("#builtin-review-dialog").open', "Rust activation")
    browser.until('document.querySelector("#language-status").dataset.state === "running"', "real rust-analyzer initialized")
    session = browser.script('return document.querySelector("#language-status").dataset.sessionId')
    browser.type('#source-editor textarea.inputarea', invalid, replace=True)
    browser.until('Number(document.querySelector("#problems-count").textContent) > 0', "real Rust draft diagnostic")
    browser.until('!!document.querySelector("#source-editor .squiggly-error")', "Monaco error marker")
    snapshot = browser.invoke("language_status", {"sessionId": session})
    document = next(d for d in snapshot["diagnostics"] if d["path"] == "src/lib.rs" and d["items"])
    assert any(item["range"]["start"]["character"] > len('pub fn answer() { let _ = "🦀";'.encode('utf-16-le')) // 2 for item in document["items"])
    assert (workspace / "src/lib.rs").read_text() == valid, "Draft diagnostics wrote source to disk"
    assert not (workspace / "build-script-executed").exists(), "Analysis executed build.rs"
    browser.click('.problem-item')
    location = document["items"][0]["range"]["start"]
    browser.until(f'document.querySelector("#cursor-position").textContent === "Ln {location["line"]+1}, Col {location["character"]+1}"', "diagnostic navigation uses UTF-16")
    browser.type('#source-editor textarea.inputarea', valid, replace=True)
    browser.until('document.querySelector("#problems-count").textContent === "0"', "markers clear on edit")
    def cleared():
        state = browser.invoke("language_status", {"sessionId": session})
        return any(d["path"] == "src/lib.rs" and d["version"] > document["version"] and not d["items"] for d in state["diagnostics"])
    wait_for(cleared, "server confirms corrected document", timeout=30)
    try:
        browser.invoke("language_sync", {"sessionId": session, "documents": [{"path": "src/lib.rs", "version": document["version"], "text": invalid}]})
    except AssertionError as error:
        assert "invalid_document" in str(error), str(error)
    else:
        raise AssertionError("LSP accepted an old document version")
    browser.click('#language-restart')
    browser.until(f'document.querySelector("#language-status").dataset.state === "running" && document.querySelector("#language-status").dataset.sessionId !== "{session}"', "Rust server restart")
    try:
        browser.invoke("language_status", {"sessionId": session})
    except AssertionError as error:
        assert "stale_session" in str(error), str(error)
    else:
        raise AssertionError("LSP accepted a stale language session")
    report["checks"].append("reviewed Rust activation, real LSP draft diagnostics, UTF-16 markers/navigation and correction")
    report["checks"].append("read-only analysis, build scripts disabled, restart and stale document/session rejection")
    servers = wait_for(lambda: rust_server_pids(driver_pid), "owned Rust server process")
    assert len(servers) == 1, servers
    os.kill(next(iter(servers)), signal.SIGKILL)
    browser.until('document.querySelector("#language-status").dataset.state === "error"', "server crash surfaced")
    assert browser.script('return document.querySelector("#problems-count").textContent') == "0"
    browser.type('#source-editor textarea.inputarea', invalid, replace=True)
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "dirty"', "editor survives server crash")
    browser.click('#language-restart')
    browser.until('document.querySelector("#language-status").dataset.state === "running" && Number(document.querySelector("#problems-count").textContent) > 0', "diagnostics resume after server crash")
    servers = wait_for(lambda: rust_server_pids(driver_pid), "restarted server process")
    report["checks"].append("real Rust server crash clears diagnostics, preserves editing and recovers on restart")

    browser.click('[data-activity="settings"]')
    browser.click('[data-plugin-toggle="io.github.w3ti.lyrnova.language.rust"]')
    browser.until('!document.querySelector("#language-enable").hidden', "Rust plugin deactivation clears analysis")
    wait_for(lambda: all(not running(pid) for pid in servers), "plugin deactivation terminates Rust server")
    assert browser.script('return document.querySelector("#problems-count").textContent') == "0"
    try:
        browser.invoke("language_start", {"workspace": str(workspace), "restart": False})
    except AssertionError as error:
        assert "permission_denied" in str(error), str(error)
    else:
        raise AssertionError("LSP retained authority after plugin deactivation")
    browser.click('[data-activity="explorer"]')
    browser.type('#source-editor textarea.inputarea', valid, replace=True)
    browser.until('document.querySelector("#editor-workspace").dataset.saveState === "clean"', "discard fixture draft by restoring saved text")
    browser.click('[data-close-editor-path="src/lib.rs"]')
    report["checks"].append("Rust plugin deactivation revokes server authority and clears diagnostics")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/lyrnova")
    parser.add_argument("--driver", default="tauri-driver")
    parser.add_argument("--native-driver", default="WebKitWebDriver")
    parser.add_argument("--fixture", type=Path, default=ROOT / "target/debug/examples/e2e_fixture")
    parser.add_argument("--rust-analyzer", type=Path, help="Standalone ELF server to exercise real Rust LSP diagnostics")
    parser.add_argument("--output", type=Path, default=ROOT / "target/e2e")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve(strict=True)
    fixture = args.fixture.resolve(strict=True)
    driver = shutil.which(args.driver)
    native = shutil.which(args.native_driver)
    if not driver or not native:
        parser.error("Install tauri-driver and WebKitWebDriver; see tests/e2e/README.md")
    report = {"ok": False, "checks": [], "binary": str(binary), "platform": platform.platform(), "inputMode": "DOM handlers in native WebView"}
    with tempfile.TemporaryDirectory(prefix="lyrnova-e2e-") as temporary:
        root = Path(temporary)
        workspace = root / "workspace"
        workspace.mkdir()
        (workspace / "README.md").write_text("initial\n")
        # Git config is isolated from the user's identity, signing and hooks.
        env = os.environ.copy()
        env = {key: value for key, value in env.items() if not key.startswith("GIT_")}
        env.update(GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1")
        subprocess.run(["git", "init", "-b", "main", str(workspace)], env=env, check=True, stdout=subprocess.DEVNULL)
        for key, value in [("user.name", "Lyrnova E2E"), ("user.email", "e2e@example.invalid"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")]:
            subprocess.run(["git", "-C", str(workspace), "config", key, value], env=env, check=True)
        subprocess.run(["git", "-C", str(workspace), "add", "README.md"], env=env, check=True)
        subprocess.run(["git", "-C", str(workspace), "commit", "-m", "Fixture"], env=env, check=True, stdout=subprocess.DEVNULL)
        config = root / "config/io.github.w3ti.lyrnova"
        config.mkdir(parents=True)
        (config / "projects.json").write_text(json.dumps({"version": 1, "recent": [str(workspace)]}))
        (root / ".lyrnova-e2e-fixture").touch()
        subprocess.run([str(fixture), str(root)], check=True)
        env.update(XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"))
        # Match the tested CI backend (XWayland locally, Xvfb in CI).
        env["GDK_BACKEND"] = "x11"
        if args.rust_analyzer:
            tools_dir = root / "tools"
            tools_dir.mkdir()
            (tools_dir / "rust-analyzer").symlink_to(args.rust_analyzer.resolve(strict=True))
            env["PATH"] = str(tools_dir) + os.pathsep + env.get("PATH", "/usr/bin:/bin")
        port, native_port = free_port(), free_port()
        while native_port == port:
            native_port = free_port()
        browser = Browser(f"http://127.0.0.1:{port}")
        with (args.output / "driver.log").open("w") as log:
            process = subprocess.Popen([driver, "--native-driver", native, "--port", str(port), "--native-port", str(native_port)], cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                def ready():
                    if process.poll() is not None:
                        raise RuntimeError("Driver exited; see driver.log")
                    try:
                        browser.request("GET", "/status")
                        return True
                    except (urllib.error.URLError, http.client.RemoteDisconnected):
                        return False
                wait_for(ready, "WebDriver startup")
                started = time.monotonic()
                report["capabilities"] = browser.start(binary)
                exercise(browser, workspace, report, started)
                report["memorySnapshot"] = process_memory(process.pid)
                browser.close()
                browser.start(binary)
                browser.until('document.querySelector("#project-name")?.textContent === "workspace"', "project after restart")
                browser.until('!!document.querySelector(\'[data-file="README.md"]\')', "tree after restart")
                browser.until('document.querySelector("#source-editor .view-lines")?.textContent.includes("unsaved")', "recovered draft after restart")
                browser.until('document.querySelector("#cursor-position").textContent === "Ln 2, Col 16"', "restored cursor")
                assert browser.script('return [...document.querySelectorAll("[data-editor-path]")].map(node => node.dataset.editorPath)') == ["README.md", "missing.txt", "clean.txt"]
                assert browser.script('return !document.querySelector("#editor-recovery").hidden')
                browser.click('[data-action="save-document"]')
                browser.until('document.querySelector("#editor-workspace").dataset.saveState === "error"', "recovered conflict blocks save")
                assert (workspace / "README.md").read_text() == "external edit\n"
                browser.click('[data-editor-path="clean.txt"]')
                browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("changed")', "clean tab reads latest disk content")
                browser.until('document.querySelector("#cursor-position").textContent.startsWith("Ln 161,")', "inactive tab cursor and scroll restored")
                browser.click('[data-editor-path="missing.txt"]')
                browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("deleted")', "deleted-file draft restored")
                assert not (workspace / "missing.txt").exists()
                browser.script('window.prompt = () => "recovered.txt"')
                browser.click('#recovery-copy')
                wait_for(lambda: (workspace / "recovered.txt").exists(), "save recovered copy")
                assert (workspace / "recovered.txt").read_text() == "draft of deleted file"
                browser.until('!!document.querySelector(\'[data-editor-path="recovered.txt"]\')', "copy opened")
                browser.click('[data-editor-path="README.md"]')
                browser.script('window.confirm = () => true')
                browser.click('#recovery-reload')
                browser.until('document.querySelector("#source-editor .view-lines").textContent.includes("external") && document.querySelector("#editor-recovery").hidden', "explicit discard rereads disk")
                assert (workspace / "executions").read_text() == "run\n"
                assert browser.invoke("ai_provider_current") is None
                report["checks"].append("restart restores tab order, active draft and cursor without replaying commands")
                report["checks"].append("changed and deleted files preserve drafts; copy and explicit discard resolve recovery")
                browser.terminal("sleep 300 & echo $! > close-child.pid")
                wait_for(lambda: (workspace / "close-child.pid").exists(), "child before closing window")
                close_pid = int((workspace / "close-child.pid").read_text())
                browser.type("#source-editor textarea.inputarea", "draft on close", replace=True)
                browser.script('setTimeout(() => document.querySelector("#window-close").click(), 100)')
                wait_for(lambda: not running(close_pid), "closing window terminates terminal descendants", timeout=5)
                report["checks"].append("normal window close terminates terminal descendants")
                browser.close()
                browser.start(binary)
                browser.until('document.querySelector("#source-editor .view-lines")?.textContent.includes("close")', "normal close flushes unsaved content")
                browser.script('window.confirm = () => true')
                paths = browser.script('return [...document.querySelectorAll("[data-close-editor-path]")].map(node => node.dataset.closeEditorPath)')
                for path in paths:
                    browser.click(f'[data-close-editor-path="{path}"]')
                browser.until('!document.querySelector("[data-editor-path]") && document.querySelector("#session-state").dataset.state === "saved"', "empty session persisted")
                browser.close()
                browser.start(binary)
                browser.until('document.querySelector("#session-state").dataset.state === "saved"', "empty session restored")
                assert browser.script('return !document.querySelector("[data-editor-path]") && document.querySelector("#editor-workspace").dataset.empty === "true"')
                report["checks"].append("normal close flushes drafts; explicitly closed tabs stay closed after restart")
                if args.rust_analyzer:
                    exercise_rust_diagnostics(browser, workspace, report, process.pid)
                else:
                    report["rustLsp"] = "not exercised; supply --rust-analyzer"

                browser.close()
                snapshots = list((root / "data").glob("*/editor-sessions-v1/*.json"))
                assert len(snapshots) == 1, snapshots
                corrupt = '{"version":999,"documents":[],"active":null}'
                snapshots[0].write_text(corrupt)
                browser.start(binary)
                browser.until('document.querySelector("#session-state").dataset.state === "error"', "unsupported session schema surfaced")
                browser.until('!!document.querySelector(\'[data-file="README.md"]\')', "editor remains available")
                browser.click('[data-file="README.md"]')
                browser.until('document.querySelector("#source-editor .view-lines")?.textContent.includes("external")', "disk content available after recovery failure")
                browser.type("#source-editor textarea.inputarea", "still editable", replace=True)
                browser.until('document.querySelector("#editor-workspace").dataset.saveState === "dirty"', "editing with unsupported session")
                assert snapshots[0].read_text() == corrupt
                report["checks"].append("unsupported session preserved with visible error and usable editor")


                report["ok"] = True
            except Exception as error:
                report["error"] = str(error) or type(error).__name__
                report["traceback"] = traceback.format_exc()
                if browser.session:
                    try:
                        (args.output / "failure.png").write_bytes(base64.b64decode(browser.command("GET", "/screenshot")))
                        (args.output / "failure.html").write_text(browser.script("return document.documentElement.outerHTML"))
                    except Exception as capture_error:
                        report["captureError"] = str(capture_error)
            finally:
                try:
                    browser.close()
                except Exception as close_error:
                    report["closeError"] = str(close_error)
                    report["ok"] = False
                # Clean up only the isolated driver group created by this runner.
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=5)
                except ProcessLookupError:
                    pass
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
                # Also clean up the explicit sleep fixture if the regression failed.
                for name in ["descendant.pid", "close-child.pid", "interrupt.pid", "flood.pid"]:
                    pid_file = workspace / name
                    if pid_file.exists():
                        pid = int(pid_file.read_text())
                        if running(pid):
                            os.kill(pid, signal.SIGKILL)
    (args.output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
