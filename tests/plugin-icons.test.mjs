import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const url=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const output=ts.transpileModule(readFileSync(new URL('../src/services/pluginIcons.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const kinds=ts.transpileModule(readFileSync(new URL('../src/services/pluginKinds.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {pluginTypeIcons}=await import(url(output.replace('from "./pluginLoader"',`from "${url('export const pluginUrl=(id,path)=>"plugin://localhost/"+id+"/"+path')}"`).replace('from "./pluginKinds"',`from "${url(kinds)}"`)));
const importer=(id,values)=>({manifest:{id,...values}});
test('navigation icons use per-type artwork with a plugin icon fallback and stable provider selection',()=>{
  const first=importer('a',{assetKinds:['model','animation'],icon:'model.svg',assetIcons:{animation:'animation.svg'}});
  const second=importer('b',{assetKinds:['model'],icon:'other.svg'});
  const expected={model:'plugin://localhost/a/model.svg',animation:'plugin://localhost/a/animation.svg'};
  assert.deepEqual(pluginTypeIcons([first,second]),expected);assert.deepEqual(pluginTypeIcons([second,first]),expected);
});
test('legacy plugins leave fallback glyphs available and icon paths cannot escape the plugin',()=>{
  for(const path of ['../other/icon.svg','https://example.test/icon.svg','/icon.svg','icons\\icon.svg','icons/../../icon.svg']) {
    assert.deepEqual(pluginTypeIcons([importer('a',{assetKinds:['audio'],icon:path})]),{});
  }
  assert.deepEqual(pluginTypeIcons([importer('old',{})]),{});
  assert.deepEqual(pluginTypeIcons([importer('old',{icon:'icon.svg'})]),{model:'plugin://localhost/old/icon.svg'});
});
test('a plugin that declares a type supplies its icon ahead of any importer for that type',()=>{
  const types=[{id:'skybox',pluginId:'z.skies',icon:'icons/sky.svg'},{id:'model',pluginId:'z.models',icon:'../escape.svg'},{id:'audio',icon:'no-plugin.svg'}];
  const skies=importer('z.skies',{assetTypes:[{id:'skybox'}],assetKinds:['skybox'],icon:'plugin.svg'});
  const other=importer('a.other',{assetKinds:['skybox','model'],icon:'other.svg'});
  assert.deepEqual(pluginTypeIcons([other,skies],types),{skybox:'plugin://localhost/z.skies/icons/sky.svg',model:'plugin://localhost/a.other/other.svg'});
  // Without a declared icon, the type falls back to its plugin's own artwork.
  assert.deepEqual(pluginTypeIcons([skies],[{id:'skybox',pluginId:'z.skies'}]),{skybox:'plugin://localhost/z.skies/plugin.svg'});
});
