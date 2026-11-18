import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const load=async path=>{const code=ts.transpileModule(readFileSync(new URL(path,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));};
const {withImportProvenance}=await load('../src/domain/importProvenance.ts');
const {applyFilter,emptyFilter,tagCounts,provenanceCounts,sourceLabel,assetFormat,formatCounts,formatLabel,isFilterActive}=await load('../src/domain/facets.ts');
test('shared provenance reaches variant files without mutating plugin entries or blanking their metadata',()=>{
  const low={id:'low',pack:'p',category:'c',file:'low.gltf',provenance:{license:'CC0'}};
  const entry={id:'full',pack:'p',category:'c',file:'full.gltf',variants:[low],provenance:{filesystemPath:'original.gltf'}};
  const result=withImportProvenance(entry,{source:' Pack ',publisher:' Studio ',license:'  '});
  assert.deepEqual(result.provenance,{filesystemPath:'original.gltf',source:'Pack',publisher:'Studio'});
  assert.deepEqual(result.variants[0].provenance,{license:'CC0',source:'Pack',publisher:'Studio'});
  assert.equal(entry.provenance.source,undefined);assert.equal(low.provenance.publisher,undefined);
});
test('provenance tags support name-independent search and exact combined tag filtering',()=>{
  const assets=[{id:'a',kind:'model',name:'Crate',pack:'p',category:'c',user:{tags:['source: Space Pack','publisher: Studio','license: CC0']}},
    {id:'b',kind:'model',name:'Barrel',pack:'p',category:'c',user:{tags:['publisher: Studio','license: CC BY 4.0']}},
    {id:'c',kind:'material',name:'Steel',pack:'p',category:'c',user:{tags:['license: CC0']}}];
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),text:'SPACE PACK'}).map(a=>a.id),['a']);
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),text:'studio'}).map(a=>a.id),['a','b']);
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),tags:new Set(['publisher: Studio','license: CC0'])}).map(a=>a.id),['a']);
  assert.equal(tagCounts(assets).find(t=>t.value==='license: CC0').count,2);
});

test('source groups span storage packs and distinguish source titles within the same pack',()=>{
  const make=(id,pack,source)=>({id,pack,name:id,kind:'model',category:'props',user:{tags:[],provenance:source?{source}:undefined}});
  const assets=[make('a','folder-a','Shared Source'),make('b','folder-b','Shared Source'),make('c','folder-a','Other Source'),make('d','not-a-source')];
  assert.deepEqual(provenanceCounts(assets,'source'),[{value:'',count:1},{value:'Other Source',count:1},{value:'Shared Source',count:2}]);
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),sources:new Set(['Shared Source'])}).map(a=>a.id),['a','b']);
  assert.equal(sourceLabel(assets[3]),'Unknown source');
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),sources:new Set([''])}).map(a=>a.id),['d']);
});

test('publisher and license filters combine with sources and custom tags; choices within a facet use OR',()=>{
  const make=(id,publisher,license)=>({id,name:id,kind:'material',category:'metal',user:{tags:['metal'],provenance:{source:'Source',publisher,license}}});
  const assets=[make('a','Studio A','CC0'),make('b','Studio B','CC0'),make('c','Studio A','CC BY')];
  const filter={...emptyFilter('material'),sources:new Set(['Source']),publishers:new Set(['Studio A','Studio B']),licenses:new Set(['CC0']),tags:new Set(['metal'])};
  assert.deepEqual(applyFilter(assets,filter).map(a=>a.id),['a','b']);
  filter.publishers.delete('Studio B');assert.deepEqual(applyFilter(assets,filter).map(a=>a.id),['a']);
});

test('assetFormat prefers the recorded format and falls back per kind to the primary file extension',()=>{
  const base={name:'n',pack:'p',category:'c',fileName:'n',user:{tags:[]}};
  const model=(id,format,gltf='library/models/k/a.gltf')=>({...base,id,kind:'model',format,fileset:{gltf,bin:'',textures:[]}});
  assert.equal(assetFormat(model('a','fbx')),'fbx');
  assert.equal(assetFormat(model('b',' .OBJ ')),'obj');
  assert.equal(assetFormat(model('c')),'gltf');
  assert.equal(assetFormat({...model('d'),kind:'animation'}),'gltf');
  assert.equal(assetFormat({...base,id:'e',kind:'audio',audio:{file:'library/audio/k/Boom.WAV'}}),'wav');
  const maps=[{role:'albedo',file:'library/materials/k/Albedo.PNG'}];
  assert.equal(assetFormat({...base,id:'f',kind:'material',material:{maps}}),'png');
  assert.equal(assetFormat({...base,id:'g',kind:'material',material:{maps},origin:{plugin:'x',files:['library/materials/k/tiles.sbsar']}}),'sbsar');
  assert.equal(assetFormat({...base,id:'h',kind:'plugin',type:'skybox',files:['library/skybox/k/sky.hdr']}),'hdr');
  assert.equal(assetFormat({...base,id:'i',kind:'plugin',type:'skybox',files:[]}),'');
  assert.equal(assetFormat({...base,id:'j',kind:'model',format:'  ',fileset:{gltf:'noextension'}}),'');
});

test('format filter keeps OR within the facet, ANDs with others, and counts only what it is given',()=>{
  const make=(id,kind,format,license)=>({id,name:id,kind,category:'c',pack:'p',fileName:id,format,fileset:{gltf:id+'.gltf'},user:{tags:[],provenance:{license}}});
  const assets=[make('a','model','fbx','CC0'),make('b','model','obj','CC0'),make('c','model','fbx','CC BY'),make('d','model',undefined,'CC0'),make('e','animation','fbx','CC0')];
  assert.deepEqual(formatCounts(assets.filter(a=>a.kind==='model')),[{value:'fbx',count:2},{value:'gltf',count:1},{value:'obj',count:1}]);
  assert.equal(formatLabel('fbx'),'FBX');assert.equal(formatLabel(''),'Unknown');
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),formats:new Set(['fbx'])}).map(a=>a.id),['a','c']);
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),formats:new Set(['fbx','obj'])}).map(a=>a.id),['a','b','c']);
  assert.deepEqual(applyFilter(assets,{...emptyFilter(),formats:new Set(['fbx','gltf']),licenses:new Set(['CC0'])}).map(a=>a.id),['a','d']);
  assert.deepEqual(applyFilter(assets,{...emptyFilter('animation'),formats:new Set(['fbx'])}).map(a=>a.id),['e']);
  assert.equal(isFilterActive({...emptyFilter(),formats:new Set(['obj'])}),true);
  assert.equal(isFilterActive(emptyFilter()),false);
});
