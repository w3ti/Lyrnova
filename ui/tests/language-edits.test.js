import test from "node:test";
import assert from "node:assert/strict";
import { prepareRenameDrafts } from "../language-edits.js";
const documents = [{ path: "lib.rs", original: "caller", edits: [{range:{start:{line:0,character:0},end:{line:0,character:6}},newText:"renamed"}] }, { path: "helper.rs", original: "target", edits: [] }];
function fixture() {
  const options = { getGeneration: () => 1, isBlocked: () => false, read: async () => ({ content: "target", revision: "disk-revision" }),
    draftDocuments: new Map([["lib.rs","caller"]]), savedDocuments: new Map([["lib.rs","disk caller"]]), documentRevisions: new Map(), recoveryConflicts: new Set(), openDocuments: ["lib.rs"],
    ensureDocumentModel: path => ({ uri:path, getVersionId:()=>42 }), onPrepared() {},
  }; return options;
}
test("rename stages closed files with disk revision but never overwrites existing unsaved drafts", async () => {
  const f = fixture(); const edits = await prepareRenameDrafts(documents, () => true, f);
  assert.equal(f.savedDocuments.get("lib.rs"), "disk caller");
  assert.equal(f.draftDocuments.get("lib.rs"), "caller");
  assert.equal(f.draftDocuments.get("helper.rs"), "target");
  assert.equal(f.documentRevisions.get("helper.rs"), "disk-revision");
  assert.deepEqual(f.openDocuments, ["lib.rs","helper.rs"]);
  assert.equal(edits[0].versionId, 42); assert.equal(edits[0].textEdit.text, "renamed");
});
for (const reason of ["read failure", "disk change", "draft change", "workspace change", "cancellation", "recovery conflict"]) {
  test(`rename stages nothing on ${reason}`, async () => {
    const f = fixture(); let current = true, generation = 1; f.getGeneration = () => generation;
    f.read = async () => {
      if (reason === "read failure") throw new Error("read failed");
      if (reason === "draft change") f.draftDocuments.set("lib.rs","user typed");
      if (reason === "workspace change") generation = 2;
      if (reason === "cancellation") current = false;
      if (reason === "recovery conflict") f.recoveryConflicts.add("lib.rs");
      return {content: reason === "disk change" ? "changed" : "target",revision:"disk"};
    };
    await assert.rejects(prepareRenameDrafts(documents, () => current, f));
    assert.equal(f.draftDocuments.has("helper.rs"), false);
    assert.deepEqual(f.openDocuments, ["lib.rs"]);
  });
}
