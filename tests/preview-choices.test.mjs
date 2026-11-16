import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const {outputText}=ts.transpileModule(readFileSync(new URL("../src/domain/previewChoices.ts",import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
const {directPreviewChoices}=await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
function fixture(){
  const rest=[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1];
  const a={id:"a",kind:"skeleton",unambiguous:true,nodes:[{key:"Root",parent:null,rest},{key:"Root/Hip",parent:"Root",rest}]};
  const b=structuredClone(a);b.id="b";
  const animation={id:"clip",kind:"animation",rigs:[{rigId:"a",nodes:{Root:8,"Root/Hip":9}}],clip:{tracks:[{node:9,path:"rotation"}]}};
  const model={id:"model",kind:"model",rigs:[{rigId:"b",nodes:{Root:0,"Root/Hip":1}}]};
  return {animation,model,catalog:{rigs:[a,b],assets:[animation,model]},b};
}
test("direct previews use named topology and rest frames despite different file-local indices",()=>{
  const {animation,model,catalog}=fixture();
  assert.deepEqual(directPreviewChoices(animation,catalog),[{assetId:"model",rigId:"b",exact:true}]);
  assert.deepEqual(directPreviewChoices(model,catalog),[{assetId:"clip",rigId:"b",exact:true}]);
});
test("extra unanimated bones are allowed but ambiguous or changed ancestors and morph tracks are rejected",()=>{
  let {animation,catalog,b}=fixture();b.nodes.push({key:"Extra",parent:null,rest:b.nodes[0].rest});
  assert.equal(directPreviewChoices(animation,catalog)[0].exact,false);
  b.unambiguous=false;assert.deepEqual(directPreviewChoices(animation,catalog),[]);
  ({animation,catalog,b}=fixture());b.nodes[0].rest[12]=1;assert.deepEqual(directPreviewChoices(animation,catalog),[]);
  ({animation,catalog}=fixture());animation.clip.tracks[0].path="weights";assert.deepEqual(directPreviewChoices(animation,catalog),[]);
});
