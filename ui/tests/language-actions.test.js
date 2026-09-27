import test from "node:test";
import assert from "node:assert/strict";
import { registerRustActions } from "../language-actions.js";
const range = { start: {line:0,character:0},end:{line:0,character:2} };
const position = {lineNumber:1,column:3};
function setup(result) {
  const providers = {}, reports = [], prepared = [], calls = [];
  const nodes = new Map();
  globalThis.document = { querySelector(selector) { if (!nodes.has(selector)) nodes.set(selector, {addEventListener(){},replaceChildren(){},close(){},open:false}); return nodes.get(selector); } };
  const monaco = { languages: { registerCodeActionProvider(_,p){providers.actions=p.provideCodeActions;providers.resolveAction=p.resolveCodeAction;}, CompletionItemInsertTextRule:{InsertAsSnippet:4}, registerCompletionItemProvider(_,p){providers.completion=p.provideCompletionItems;},registerDocumentFormattingEditProvider(_,p){providers.format=p.provideDocumentFormattingEdits;},registerRenameProvider(_,p){providers.rename=p.provideRenameEdits;} } };
  registerRustActions({monaco, query:async(...args)=>{calls.push(args);return typeof result === "function" ? result(...args) : result;}, rangeFor:r=>r, report:text=>reports.push(text), prepareRename:async(...args)=>{prepared.push(args);return [{versionId:7}];} });
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

test("auto-imports are fully resolved before suggestions become acceptable, with bounded parallelism", async()=>{
  let active=0,peak=0;
  const item={label:"target",insertText:"target()",kind:3,additional:[],range};
  const f=setup(async kind=>{
    if(kind==="completion") return {kind,value:{items:Array.from({length:40},(_,i)=>({...item,resolveId:String(i)})),incomplete:false},isCurrent:()=>true};
    peak=Math.max(peak,++active); await new Promise(resolve=>setTimeout(resolve,1)); active--;
    return {kind:"completion",value:{items:[{...item,additional:[{range,newText:"use crate::target;"}]}]},isCurrent:()=>true};
  });
  const result=await f.providers.completion(f.model,position,{},{});
  assert.equal(result.suggestions.length,32); assert.equal(result.incomplete,true); assert.ok(peak<=4);
  assert.ok(result.suggestions.every(i=>i.additionalTextEdits[0].text==="use crate::target;"));
});
test("failed or cancelled import resolution never offers a partially resolved suggestion",async()=>{
  for(const cancel of [false,true]) {
    const token={isCancellationRequested:false};
    const f=setup(kind=>{
      if(kind==="completion") return {kind,value:{items:[{label:"target",resolveId:"opaque",additional:[]}]},isCurrent:()=>true};
      token.isCancellationRequested=cancel; return null;
    });
    const result=await f.providers.completion(f.model,position,{},token);
    if(cancel) assert.equal(result,null); else {assert.equal(result.suggestions.length,0);assert.equal(result.incomplete,true);}
  }
});
test("quick fixes defer draft creation until selection and reject stale or cancelled selection",async()=>{
  for(const mode of ["current","stale","cancelled"]) {
    let current=true;
    const edits=[{path:"lib.rs",original:"old",edits:[]}];
    const f=setup({kind:"code_action",value:[{title:"Import target",preferred:true,edits}],isCurrent:()=>current});
    const actions=await f.providers.actions(f.model,{startLineNumber:1,startColumn:1,endLineNumber:1,endColumn:3},{},{});
    assert.equal(f.prepared.length,0); assert.equal(actions.actions[0].command,undefined);
    current=mode!=="stale";
    const resolved=await f.providers.resolveAction(actions.actions[0],{isCancellationRequested:mode==="cancelled"});
    assert.deepEqual(resolved.edit.edits,mode==="current"?[{versionId:7}]:[]);
    assert.equal(f.prepared.length,mode==="current"?1:0);
  }
});
