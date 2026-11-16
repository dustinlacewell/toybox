import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import ts from "typescript";
const url=source=>`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const paths=url(`export const assetUrl=file=>globalThis.auditionPaths.get(file).promise;`);
const {outputText}=ts.transpileModule(readFileSync(new URL("../src/services/audioAudition.ts",import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
const audition=await import(url(outputText.replace('from "./assetUrl"',`from "${paths}"`)));
const players=[];
globalThis.Audio=class {
  paused=true;currentTime=0;playCalls=0;
  constructor(){players.push(this);}
  pause(){this.paused=true;}
  removeAttribute(name){delete this[name];}
  load(){this.currentTime=0;}
  async play(){this.playCalls++;this.paused=false;}
};
function deferred(){let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject};}
test("switching cues during path loading prevents stale audio and keeps only one audition active",async()=>{
  const a=deferred(),b=deferred();globalThis.auditionPaths=new Map([["a.wav",a],["b.wav",b]]);
  const first=audition.toggleAudioAudition("a","a.wav");const old=players.at(-1);
  const second=audition.toggleAudioAudition("b","b.wav");const current=players.at(-1);
  assert.equal(old.paused,true);a.resolve("url:a");await first;assert.equal(old.playCalls,0);
  b.resolve("url:b");await second;assert.equal(current.playCalls,1);assert.equal(current.paused,false);assert.equal(audition.getAudioAudition().assetId,"b");
  await audition.toggleAudioAudition("b","b.wav");assert.equal(current.paused,true);assert.equal(current.currentTime,0);assert.equal(audition.getAudioAudition().status,"idle");
});
test("failed and finished cues release the player and permit another audition",async()=>{
  const failed=deferred();globalThis.auditionPaths=new Map([["bad.wav",failed]]);
  const attempt=audition.toggleAudioAudition("bad","bad.wav");failed.reject(Error("Missing audio"));await attempt;
  assert.equal(audition.getAudioAudition().status,"error");assert.equal(players.at(-1).paused,true);
  const retry=deferred();globalThis.auditionPaths.set("bad.wav",retry);const playing=audition.toggleAudioAudition("bad","bad.wav");retry.resolve("url:fixed");await playing;
  assert.equal(audition.getAudioAudition().status,"playing");players.at(-1).onended();assert.equal(audition.getAudioAudition().status,"idle");assert.equal(players.at(-1).paused,true);
});
