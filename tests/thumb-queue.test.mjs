import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const url = source => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const native = url(`export async function saveThumb(id){globalThis.thumbHarness.saved.push(id);} export async function setThumbState(id,state){globalThis.thumbHarness.failed.push([id,state]);}`);
const renderer = url(`export class ThumbRenderer {
  constructor(){this.h=globalThis.thumbHarness;this.h.renderers++;}
  async prepare(asset){const h=this.h;h.preparing++;h.maxPreparing=Math.max(h.maxPreparing,h.preparing);h.started.push(asset.id);try{await h.gates.get(asset.id).promise;if(h.reject===asset.id)throw Error('Broken source');return {id:asset.id};}finally{h.preparing--;}}
  async renderPrepared(subject){const h=this.h;h.rendering++;h.maxRendering=Math.max(h.maxRendering,h.rendering);try{await new Promise(r=>setTimeout(r,1));return new Uint8Array([1]);}finally{h.rendering--;h.released.push(subject.id);}}
  discard(subject){this.h.released.push(subject.id);}
  dispose(){this.h.disposed++;}
}`);
let {outputText}=ts.transpileModule(readFileSync(new URL("../src/services/thumbQueue.ts",import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
outputText=outputText.replace('from "./tauriApi"',`from "${native}"`).replace('from "./thumbRenderer"',`from "${renderer}"`);
const {ThumbQueue}=await import(url(outputText));
globalThis.window={};
const flush=()=>new Promise(resolve=>setTimeout(resolve,10));
function setup(n=7){
  const assets=Array.from({length:n},(_,i)=>({id:String(i),name:String(i)}));
  const h={saved:[],failed:[],released:[],started:[],preparing:0,maxPreparing:0,rendering:0,maxRendering:0,renderers:0,disposed:0,gates:new Map(assets.map(a=>{let resolve;const promise=new Promise(r=>resolve=r);return [a.id,{promise,resolve}];}))};
  globalThis.thumbHarness=h;
  const q=new ThumbQueue(new Map(assets.map(a=>[a.id,a])),assets.map(a=>a.id),{onProgress:p=>h.progress=p,onAssetState:()=>{}});
  return {h,q,release:()=>{for(const gate of h.gates.values())gate.resolve();}};
}
test("thumbnail preparation overlaps three loads while captures remain ordered on one renderer",async()=>{
  const {h,q,release}=setup();const running=q.start();await flush();
  assert.deepEqual(h.started,["0","1","2"]);assert.equal(h.maxPreparing,3);assert.deepEqual(h.saved,[]);
  release();await running;
  assert.deepEqual(h.saved,["0","1","2","3","4","5","6"]);assert.equal(h.maxRendering,1);assert.equal(h.renderers,1);assert.equal(h.disposed,1);
  assert.equal(h.released.length,7);assert.equal(new Set(h.released).size,7);
});
test("pause drains unused preparations, then resume regenerates them without skipping or leaking",async()=>{
  const {h,q,release}=setup(5);const running=q.start();await flush();q.pause();release();await running;
  assert.deepEqual(h.saved,[]);assert.equal(h.progress.done,0);assert.equal(h.released.length,3);assert.equal(h.disposed,0);
  await q.start();assert.deepEqual(h.saved,["0","1","2","3","4"]);assert.equal(h.disposed,1);assert.equal(h.released.length,8);
});
test("teardown waits for in-flight preparations before disposing the renderer",async()=>{
  const {h,q,release}=setup();const running=q.start();await flush();const stopping=q.teardown();
  assert.equal(h.disposed,0);release();await Promise.all([running,stopping]);
  assert.deepEqual(h.saved,[]);assert.equal(h.released.length,3);assert.equal(h.disposed,1);
});
test("a failed preparation is recorded and does not strand subsequent thumbnails",async()=>{
  const {h,q,release}=setup(4);h.reject="1";release();await q.start();
  assert.deepEqual(h.failed,[["1","error"]]);assert.deepEqual(h.saved,["0","2","3"]);assert.equal(h.disposed,1);assert.equal(h.progress.done,4);
});
