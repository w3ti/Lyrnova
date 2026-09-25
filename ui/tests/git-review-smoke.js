// Injected only by the smoke harness. The actual production bundle is unmodified.
const originalStatus = {
  branch: "main", commit: "12345678", upstream: null, ahead: 0, behind: 0,
  changes: [
    { path: "file.txt", previousPath: null, index: "modified", worktree: "modified" },
    { path: "removed.txt", previousPath: null, index: null, worktree: "deleted" },
  ],
};
const test = window.gitSmoke = {
  status: structuredClone(originalStatus), calls: [], reviewNumber: 0,
  slowDiff: false, slowReview: false, rejectCommit: false,
};
const patch = (word) => `diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-base\n+${word}\n`;
window.__TAURI__ = {
  event: { listen: async () => () => {} },
  core: { invoke: async (command, args = {}) => {
    test.calls.push({ command, args });
    if (command === "editor_session_load") return { token: "missing", session: null };
    if (command === "editor_session_save") return "saved";
    if (command === "project_current") return { name: "Projeto de teste", path: "/synthetic", hasGit: true };
    if (["workspace_list", "plugin_list", "plugin_catalog_list"].includes(command)) return [];
    if (command === "ai_provider_current") return null;
    if (command === "task_list") return { items: [], sandbox: { isolatedNetwork: "strong" } };
    if (command === "terminal_start") return { sessionId: "smoke-terminal", cols: 80, rows: 12 };
    if (["terminal_resize", "terminal_ack"].includes(command)) return null;
    if (command === "git_status") return structuredClone(test.status);
    if (command === "git_diff") {
      if (test.slowDiff) {
        test.slowDiff = false;
        return new Promise((resolve) => { test.releaseDiff = () => resolve({ patch: patch("STALE"), truncated: false }); });
      }
      return { patch: patch(args.path === "removed.txt" ? "DELETED" : args.scope === "index" ? "STAGED" : "WORKTREE"), truncated: false };
    }
    if (command === "git_commit_review") {
      const review = { token: `review-${++test.reviewNumber}`, branch: "main", message: args.message, files: 1, diff: { patch: patch("STAGED"), truncated: false } };
      if (test.slowReview) {
        test.slowReview = false;
        return new Promise((resolve) => { test.releaseReview = () => resolve(review); });
      }
      return review;
    }
    if (command === "git_commit_review_discard") return null;
    if (command === "git_commit") {
      if (test.rejectCommit) throw { code: "review_changed" };
      test.status.changes = [];
      return structuredClone(test.status);
    }
    throw new Error(`Unexpected IPC: ${command}`);
  } },
};

const pause = (ms = 40) => new Promise((resolve) => setTimeout(resolve, ms));
const find = (selector) => document.querySelector(selector);
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const until = async (condition, message) => {
  for (let i = 0; i < 150; i++) { if (condition()) return; await pause(); }
  throw new Error(`Timed out: ${message}`);
};
const click = (selector) => { const node = find(selector); assert(node, `Missing: ${selector}`); node.click(); };
const displayedDiff = () => find("#git-diff-editor .view-lines")?.textContent ?? "";
const readyReview = () => find("#git-commit-review-dialog").open && !find("#git-review-confirm").disabled;
const action = (name) => `[data-action="${name}"]`;

window.addEventListener("load", async () => {
  try {
    requestAnimationFrame(() => { test.painted = true; });
    await until(() => find("#git-branch-name")?.textContent === "main", "Git status");
    click(action("show-changes"));
    await until(() => displayedDiff().includes("WORKTREE"), "worktree diff");
    assert(!find("#inspector").textContent.includes("12 arquivos alterados"), "Demo data leaked");
    test.slowDiff = true;
    click('#diff-files [data-git-diff-path="file.txt"]');
    await until(() => test.releaseDiff, "pending diff");
    click('#diff-files [data-git-diff-path="removed.txt"]');
    await until(() => displayedDiff().includes("DELETED"), "deleted file selectable");
    test.releaseDiff(); await pause(100);
    assert(displayedDiff().includes("DELETED") && !displayedDiff().includes("STALE"), "Old diff replaced selection");
    find("#diff-scope").value = "index";
    find("#diff-scope").dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => displayedDiff().includes("STAGED"), "staged scope");

    const message = find("#git-commit-message");
    message.value = "Revisão <img src=x onerror=alert(1)>";
    message.dispatchEvent(new Event("input", { bubbles: true }));
    click(action("git-commit")); await until(readyReview, "review dialog");
    assert(test.calls.filter((call) => call.command === "git_commit").length === 0, "Review executed a commit");
    assert(find("#git-review-message").textContent === message.value, "Message changed");
    assert(!find("#git-review-message img"), "Message interpreted as HTML");
    window.webkit.messageHandlers.testResult.postMessage(JSON.stringify({ snapshot: true }));
    await pause(150);
    find("#git-commit-review-dialog").dispatchEvent(new Event("cancel", { cancelable: true }));
    assert(!find("#git-commit-review-dialog").open, "Escape did not dismiss review");
    assert(test.calls.some((call) => call.command === "git_commit_review_discard"), "Canceled review not discarded");

    test.slowReview = true;
    click(action("git-commit"));
    await until(() => test.releaseReview, "pending review");
    click('#git-commit-review-dialog [data-action="cancel-git-review"]');
    test.releaseReview(); await pause(100);
    assert(!find("#git-commit-review-dialog").open && find("#git-review-confirm").disabled, "Late review reactivated confirmation");

    test.rejectCommit = true;
    click(action("git-commit")); await until(readyReview, "second review");
    click(action("confirm-git-commit"));
    await until(() => !find("#git-review-reload").hidden, "changed review error");
    assert(find("#git-review-confirm").disabled, "Changed review still confirmable");
    assert(find("#git-review-note").textContent.includes("mudaram"), "Missing stale review explanation");
    test.rejectCommit = false;
    click(action("reload-git-review")); await until(readyReview, "refreshed review");
    click(action("confirm-git-commit"));
    await until(() => !find("#git-commit-review-dialog").open, "commit complete");
    const commit = test.calls.filter((call) => call.command === "git_commit").at(-1);
    assert(Object.keys(commit.args).join() === "token", "Commit accepted frontend content instead of review token");
    assert(message.value === "", "Message retained after success");
    await until(() => find("#diff-note").textContent.includes("Nenhuma alteração"), "clean repository");
    assert(displayedDiff() === "", "Old patch retained in clean repository");
    window.webkit.messageHandlers.testResult.postMessage(JSON.stringify({ ok: true, checks: 15 }));
  } catch (error) {
    window.webkit.messageHandlers.testResult.postMessage(JSON.stringify({
      ok: false, error: String(error), stack: error.stack,
      renderedFrame: Boolean(test.painted), visibility: document.visibilityState,
    }));
  }
});
