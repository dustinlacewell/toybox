import test from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import ts from "typescript";
function moduleUrl(file,replacements={}) {
  let {outputText}=ts.transpileModule(readFileSync(new URL(file,import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX}});
  for(const [from,to]of Object.entries(replacements))outputText=outputText.replaceAll(`from "${from}"`,`from ${JSON.stringify(to)}`);
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
import {validSegment} from "@ldlework/toybox-sdk";
const {audioId,findAudio}=await import(moduleUrl("../plugins/com.toybox.audio-core/lib/files.ts"));
test("audio scans preserve nested paths, recognize supported formats and keep same-name sounds distinct",async()=>{
  const tree={".":[{name:"step.WAV",relPath:"step.WAV",isDir:false},{name:"room",relPath:"room",isDir:true},{name:"photo.png",relPath:"photo.png",isDir:false}],room:[{name:"step.wav",relPath:"room/step.wav",isDir:false},{name:"wind.ogg",relPath:"room/wind.ogg",isDir:false},{name:"music.flac",relPath:"room/music.flac",isDir:false},{name:"radio.mp3",relPath:"room/radio.mp3",isDir:false}]};
  const found=await findAudio({readDir:async(_source,path)=>tree[path]},"source");assert.equal(found.length,5);assert.ok(found.includes("room/step.wav"));
  assert.notEqual(audioId("p","sfx","step.wav"),audioId("p","sfx","room/step.wav"));assert.equal(audioId("p","sfx","step.wav"),audioId("p","sfx","step.wav"));
  for(const path of ["", "..","a/b","C:","bad\\name","trailing."])assert.equal(validSegment(path),false);
  assert.equal(validSegment("Sound effects"),true);
});
const {waveformPath}=await import(moduleUrl("../src/ds/waveformPath.ts"));
test("waveform drawing retains silence, caps peaks and fits the signal inside its viewport",()=>{
  assert.equal(waveformPath([]),"");const path=waveformPath([0,.5,5]);
  assert.equal((path.match(/M/g)??[]).length,3);assert.ok(path.includes(" 99h"));assert.ok(path.includes("v180"));assert.ok(!/NaN|Infinity/.test(path));
});
const react={"react/jsx-runtime":import.meta.resolve("react/jsx-runtime")};
const {AudioTransport}=await import(moduleUrl("../src/ds/AudioTransport.tsx",{...react,"lucide-react":import.meta.resolve("lucide-react"),"./Button":moduleUrl("../src/ds/Button.tsx",react),"./IconButton":moduleUrl("../src/ds/IconButton.tsx",react)}));
function find(element,predicate){if(!element||typeof element!=="object")return;return predicate(element)?element:[element.props?.children].flat(Infinity).map(child=>find(child,predicate)).find(Boolean);}
test("audio transport emits playback, volume, seek and loop intent without owning playback state",()=>{
  const calls=[];const control=AudioTransport({playing:false,position:2,duration:10,volume:.7,loop:false,onToggle:()=>calls.push("play"),onSeek:value=>calls.push(["seek",value]),onVolumeChange:value=>calls.push(["volume",value]),onLoopChange:value=>calls.push(["loop",value])});
  find(control,e=>e.props?.variant==="primary").props.onClick();find(control,e=>e.props?.label==="Restart").props.onClick();
  const volume=find(control,e=>e.type==="input");volume.props.onChange({currentTarget:{value:".3"}});
  const loop=find(control,e=>e.props?.["aria-pressed"]===false);loop.props.onClick();
  assert.deepEqual(calls,["play",["seek",0],["volume",.3],["loop",true]]);assert.equal(volume.props.value,.7);assert.equal(loop.props["aria-pressed"],false);
});
