import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const url=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const native=url(`export async function clearThumbs(ids,preserve){const h=globalThis.jobsHarness;h.invalidations.push({ids,preserve});if(!preserve)for(const id of ids)h.cache.delete(id);}export async function saveThumb(id,png){const h=globalThis.jobsHarness;h.cache.set(id,png[0]);h.saved.push(id);}export async function setThumbState(){}`);
const renderer=url(`export class ThumbRenderer {
 constructor(){this.h=globalThis.jobsHarness;this.h.live++;this.h.maxLive=Math.max(this.h.live,this.h.maxLive);}
 async prepare(asset){const h=this.h;h.prepared.push(asset.id);const version=h.version;if(h.block){const block=h.block;h.block=null;await block;}return {id:asset.id,version};}
 async renderPrepared(subject){return new Uint8Array([subject.version]);}
 discard(){} dispose(){this.h.live--;}
}`);
function module(path,replacements){let code=ts.transpileModule(readFileSync(new URL(path,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;for(const [from,to]of Object.entries(replacements))code=code.replaceAll(`from "${from}"`,`from "${to}"`);return url(code);}
const queue=module('../src/services/thumbQueue.ts',{'./tauriApi':native,'./thumbRenderer':renderer});
const {ThumbnailJobs}=await import(module('../src/services/thumbnailJobs.ts',{'./tauriApi':native,'./thumbQueue':queue}));
globalThis.window={};
const wait=async condition=>{for(let i=0;i<100;i++){if(condition())return;await new Promise(resolve=>setTimeout(resolve,5));}throw Error('Timed out');};
function fixture(){const h={live:0,maxLive:0,prepared:[],invalidations:[],saved:[],cache:new Map([['a',1]]),version:2,errors:[]};globalThis.jobsHarness=h;const jobs=new ThumbnailJobs({onProgress:p=>h.progress=p,onAssetState:()=>{}},error=>h.errors.push(error));jobs.updateAssets([{id:'a',name:'A'},{id:'b',name:'B'}]);return {h,jobs};}

test('newly discovered thumbnails join an active worker without overlapping renderers',async()=>{
 const {h,jobs}=fixture();let release;h.block=new Promise(resolve=>release=resolve);
 jobs.enqueue(['a']);await wait(()=>h.prepared.length===1);jobs.enqueue(['b']);release();
 await wait(()=>h.saved.length===2);await jobs.stop();
 assert.deepEqual(h.saved,['a','b']);assert.equal(h.maxLive,1);assert.equal(h.live,0);assert.deepEqual(h.errors,[]);
});
test('refresh drains obsolete preparations, preserves old PNGs, and publishes the new preference',async()=>{
 const {h,jobs}=fixture();let release;h.block=new Promise(resolve=>release=resolve);
 jobs.enqueue(['a']);await wait(()=>h.prepared.length===1);
 h.version=3;const refreshing=jobs.regenerate(['a']);await new Promise(resolve=>setTimeout(resolve,5));
 assert.equal(h.cache.get('a'),1);assert.deepEqual(h.invalidations,[]);
 release();await refreshing;await wait(()=>h.saved.length===1);await jobs.stop();
 assert.deepEqual(h.invalidations,[{ids:['a'],preserve:true}]);assert.equal(h.cache.get('a'),3);assert.equal(h.maxLive,1);assert.deepEqual(h.errors,[]);
});
test('pause holds newly discovered work until an explicit resume',async()=>{
 const {h,jobs}=fixture();jobs.pause();jobs.enqueue(['b']);await new Promise(resolve=>setTimeout(resolve,5));assert.deepEqual(h.prepared,[]);
 jobs.enqueue([],true);await wait(()=>h.saved.length===1);await jobs.stop();assert.deepEqual(h.saved,['b']);
});
