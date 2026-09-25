import test from "node:test";
import assert from "node:assert/strict";
import { createWorkspaceMonitor, reconcileDocument } from "../workspace-watch.js";
function fixture() {
  const state = { revision:"old", draft:"original", saved:"original", conflict:false, exists:true, busy:false, identity:{}, current:true };
  const options = {
    read: async () => ({revision:"new",content:"external"}), exists:()=>state.exists, busy:()=>state.busy,
    revision:()=>state.revision, identity:()=>state.identity, draft:()=>state.draft, dirty:()=>state.conflict || state.draft!==state.saved,
    accept:(_path,snapshot,replace)=>{ state.saved=snapshot.content; state.revision=snapshot.revision; state.conflict=false; if(replace)state.draft=snapshot.content; },
    conflict:()=>{state.conflict=true;},
  };
  return {state, options, run:()=>reconcileDocument("file.rs",()=>state.current,options)};
}
test("clean tabs reload, dirty tabs preserve their draft, deleted tabs become recoverable",async()=>{
  const clean=fixture(); await clean.run(); assert.equal(clean.state.draft,"external"); assert.equal(clean.state.revision,"new");
  const dirty=fixture(); dirty.state.draft="typed"; await dirty.run(); assert.equal(dirty.state.draft,"typed"); assert.equal(dirty.state.revision,"old"); assert.equal(dirty.state.conflict,true);
  const removed=fixture(); removed.options.read=async()=>{throw {code:"not_a_file"};}; await removed.run(); assert.equal(removed.state.draft,"original"); assert.equal(removed.state.conflict,true);
});
test("typing while disk read is pending never overwrites the new draft",async()=>{
  const f=fixture(); f.options.read=async()=>{f.state.draft="typed during read";return {revision:"new",content:"external"};};
  await f.run(); assert.equal(f.state.draft,"typed during read"); assert.equal(f.state.conflict,true);
});
for(const change of ["workspace","close","reopen","save","saving"]) test(`late read is ignored on ${change}`,async()=>{
  const f=fixture(); f.options.read=async()=>{
    if(change==="workspace")f.state.current=false;
    if(change==="close")f.state.exists=false;
    if(change==="reopen")f.state.identity={};
    if(change==="save")f.state.revision="saved meanwhile";
    if(change==="saving")f.state.busy=true;
    return {revision:"new",content:"external"};
  };
  await f.run(); assert.equal(f.state.draft,"original"); assert.equal(f.state.conflict,false);
});
test("save acknowledgements and restored originals clear conflicts without replacing drafts",async()=>{
  const f=fixture(); f.state.draft="saved content"; f.state.conflict=true;
  f.options.read=async()=>({revision:"new",content:"saved content"}); await f.run(); assert.equal(f.state.conflict,false); assert.equal(f.state.revision,"new");
  f.state.draft="additional typing"; await f.run(); assert.equal(f.state.draft,"additional typing"); assert.equal(f.state.conflict,false);
});
function monitorFixture() {
  let context={workspace:"/a"}, response={token:"1",resync:true,changes:[],rustChanged:true}, deferred=null;
  const calls=[], reconciled=[], refreshed=[], invalidated=[], statuses=[];
  let accept=false;
  const monitor=createWorkspaceMonitor({interval:60000, invoke:async(_command,args)=>{calls.push(args); return deferred ? deferred() : response;},context:()=>context,ready:()=>true,paths:()=>["file.rs"],reconcile:async(path)=>{reconciled.push(path);return accept;},refresh:async()=>refreshed.push(true),invalidateRust:()=>invalidated.push(true),status:(...args)=>statuses.push(args)});
  monitor.start();
  return {monitor,calls,reconciled,refreshed,invalidated,statuses,setResponse:v=>response=v,setDeferred:v=>deferred=v,accept:()=>accept=true, switch:()=>{context={workspace:"/b"};monitor.start();}};
}
test("skipped saves remain pending after a cursor is acknowledged and no more events arrive",async()=>{
  const f=monitorFixture();try{
    await f.monitor.pollNow(); f.accept(); f.setResponse({token:"1",resync:false,changes:[],rustChanged:false});
    await f.monitor.pollNow(); assert.equal(f.calls[1].token,"1"); assert.deepEqual(f.reconciled,["file.rs","file.rs"]);
    await f.monitor.pollNow(); assert.equal(f.reconciled.length,2); assert.equal(f.invalidated.length,1);
  }finally{f.monitor.stop();}
});
test("a stale workspace poll cannot reload tabs or restart the new analysis",async()=>{
  const f=monitorFixture();let finish; f.setDeferred(()=>new Promise(resolve=>{finish=resolve;}));
  try { const pending=f.monitor.pollNow(); f.switch(); finish({token:"old",resync:true,changes:[],rustChanged:true}); await pending; assert.equal(f.reconciled.length,0); assert.equal(f.invalidated.length,0); } finally{f.monitor.stop();}
});
test("failed polling retains the cursor and retries, with a visible limit error",async()=>{
  const f=monitorFixture();try{
    f.accept(); await f.monitor.pollNow(); f.setDeferred(()=>{throw {code:"too_large"};}); await f.monitor.pollNow();
    assert.equal(f.statuses.at(-1)[1],"error"); f.setDeferred(null); f.setResponse({token:"1",resync:false,changes:[],rustChanged:false}); await f.monitor.pollNow(); assert.equal(f.calls.at(-1).token,"1"); assert.equal(f.statuses.at(-1)[1],"watching");
  }finally{f.monitor.stop();}
});
