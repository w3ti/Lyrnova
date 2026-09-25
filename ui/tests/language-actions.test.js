import test from "node:test";
import assert from "node:assert/strict";
import { registerRustActions } from "../language-actions.js";
const range = { start: {line:0,character:0},end:{line:0,character:2} };
const position = {lineNumber:1,column:3};
function setup(result) {
  const providers = {}, reports = [], prepared = [], calls = [];
  const nodes = new Map();
  globalThis.document = { querySelector(selector) { if (!nodes.has(selector)) nodes.set(selector, {addEventListener(){},replaceChildren(){},close(){},open:false}); return nodes.get(selector); } };
  const monaco = { languages: { CompletionItemInsertTextRule:{InsertAsSnippet:4}, registerCompletionItemProvider(_,p){providers.completion=p.provideCompletionItems;},registerDocumentFormattingEditProvider(_,p){providers.format=p.provideDocumentFormattingEdits;},registerRenameProvider(_,p){providers.rename=p.provideRenameEdits;} } };
  registerRustActions({monaco, query:async(...args)=>{calls.push(args);return result;}, rangeFor:r=>r, report:text=>reports.push(text), prepareRename:async(...args)=>{prepared.push(args);return [{versionId:7}];} });
  const model = {getWordUntilPosition:()=>({startColumn:1,endColumn:3})};
  return {providers,reports,prepared,calls,model};
}
test("semantic completion preserves snippet/range and forwards no command or server markdown", async () => {
  const item = {label:"target",detail:"<img>",kind:3,insertText:"target(${1:x})",snippet:true,range,additional:[],sortText:"0",filterText:"target",command:{id:"evil"}};
  const f=setup({kind:"completion",value:{items:[item],incomplete:true},isCurrent:()=>true});
  const result=await f.providers.completion(f.model,position,{},{});
  assert.equal(result.suggestions[0].kind,1); assert.equal(result.suggestions[0].insertTextRules,4);
  assert.equal(result.suggestions[0].command,undefined); assert.equal(result.suggestions[0].documentation,undefined);
  assert.equal(result.incomplete,true);
});
test("formatting returns text edits while stale formatting leaves the document untouched",async()=>{
  for(const current of [true,false]) {
    const f=setup({kind:"formatting",value:[{range,newText:"formatted"}],isCurrent:()=>current});
    const result=await f.providers.format(f.model,{},{});
    assert.deepEqual(result,current?[{range,text:"formatted"}]:null);
  }
});
test("rename prepares every result before returning versioned edits; stale results never prepare",async()=>{
  for(const current of [true,false]) {
    const value=[{path:"lib.rs",original:"old",edits:[]}];
    const f=setup({kind:"rename",value,isCurrent:()=>current});
    const result=await f.providers.rename(f.model,position,"new_name",{});
    assert.deepEqual(f.calls[0][4],{newName:"new_name"});
    assert.equal(f.prepared.length,current?1:0);
    if(current) assert.deepEqual(result.edits,[{versionId:7}]); else assert.ok(result.rejectReason);
  }
});
