import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import path from 'node:path';
import ts from 'typescript';
import {createFs,loadPlugins,tempDir} from './support/cli-host.mjs';
const load=async p=>{const code=ts.transpileModule(readFileSync(new URL(p,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));};
const {withOrigin}=await load('../src/domain/origin.ts');
const {qualifyAsset,unqualifyAsset,unqualifyEntry}=await load('../src/domain/libraryId.ts');
const {collectDerived,derivedOnly,isDerivedFrom}=await load('../src/domain/derived.ts');
const {snapValue,stepDecimals,parseSliderText,formatSliderValue,sliderGesture,IDLE_GESTURE}=await load('../src/ds/sliderValue.ts');

test('withOrigin keeps the item an entry was made from, on variants too',()=>{
  const entry={kind:'material',id:'m',pack:'p',category:'c',name:'m',maps:[],origin:{derivedFrom:'lib::uid://substance_1',meta:{graph:0}},variants:[{kind:'material',id:'m_1k',pack:'p',category:'c',name:'m_1k',maps:[],origin:{derivedFrom:'lib::uid://substance_1'}}]};
  const stamped=withOrigin(entry,'com.toybox.material-sbsar');
  assert.deepEqual(stamped.origin,{plugin:'com.toybox.material-sbsar',meta:{graph:0},derivedFrom:'lib::uid://substance_1'});
  assert.deepEqual(stamped.variants[0].origin,{plugin:'com.toybox.material-sbsar',derivedFrom:'lib::uid://substance_1'});
  assert.deepEqual(withOrigin({kind:'audio',id:'s',pack:'p',category:'c',file:'s.wav',origin:{}},'x').origin,{plugin:'x'},'no empty derivedFrom is added');
});

test('derivedFrom is qualified with its library on the way in and stripped on the way out',()=>{
  const raw={kind:'material',id:'uid://sbsar_a',name:'a',user:{favorite:false,tags:[]},origin:{plugin:'p',derivedFrom:'uid://substance_1'},variants:[{kind:'material',id:'uid://sbsar_a_1k',name:'a',user:{favorite:false,tags:[]},origin:{plugin:'p',derivedFrom:'uid://substance_1'}}]};
  const qualified=qualifyAsset(raw,'lib');
  assert.equal(qualified.origin.derivedFrom,'lib::uid://substance_1');
  assert.equal(qualified.variants[0].origin.derivedFrom,'lib::uid://substance_1');
  assert.equal(raw.origin.derivedFrom,'uid://substance_1','the backend copy is not changed');
  assert.equal(qualifyAsset(qualified,'lib').origin.derivedFrom,'lib::uid://substance_1','qualifying twice does not double the prefix');
  assert.deepEqual(unqualifyAsset(qualified,'lib'),raw);
  const entry=unqualifyEntry('lib',{id:'lib::uid://sbsar_a',origin:{derivedFrom:'lib::uid://substance_1'},variants:[{id:'lib::uid://sbsar_a_1k',origin:{derivedFrom:'lib::uid://substance_1'}}]});
  assert.deepEqual(entry,{id:'uid://sbsar_a',origin:{derivedFrom:'uid://substance_1'},variants:[{id:'uid://sbsar_a_1k',origin:{derivedFrom:'uid://substance_1'}}]});
  assert.equal(unqualifyEntry('lib',{id:'x',origin:{derivedFrom:'uid://substance_1'}}).origin.derivedFrom,'uid://substance_1','a raw reference stays raw');
});

test('the derived list keeps only the parent\'s own children, whatever the server answered',()=>{
  const assets=[{id:'lib::a',origin:{derivedFrom:'lib::s'}},{id:'lib::b',origin:{plugin:'p'}},{id:'lib::c'},{id:'lib::d',origin:{derivedFrom:'lib::other'}},{id:'lib::e',origin:{derivedFrom:'lib::s'}}];
  assert.deepEqual(derivedOnly(assets,'lib::s').map(a=>a.id),['lib::a','lib::e']);
  assert.equal(isDerivedFrom({origin:{derivedFrom:''}},''),false);
  assert.equal(isDerivedFrom({origin:{derivedFrom:'s'}},'lib::s'),false,'raw and qualified ids are not mixed');
});

test('the derived list reads every page, even one with an item that does not belong',async()=>{
  const items=[...Array(7)].map((_,i)=>({id:`lib::${i}`,origin:i===1?{}:{derivedFrom:'lib::s'}}));
  const asked=[];
  const page=async(offset,limit)=>{asked.push(offset);return items.slice(offset,offset+limit);};
  assert.deepEqual((await collectDerived(page,'lib::s',3)).map(a=>a.id),['lib::0','lib::2','lib::3','lib::4','lib::5','lib::6']);
  assert.deepEqual(asked,[0,3,6]);
  asked.length=0;
  assert.deepEqual(await collectDerived(async(offset,limit)=>{asked.push(offset);return items.slice(0,6).slice(offset,offset+limit);},'lib::none',3),[]);
  assert.deepEqual(asked,[0,3,6],'an exact multiple of the page size ends on an empty page');
});

test('slider values stay in range, on the step grid, and read typed numbers',()=>{
  assert.equal(stepDecimals(1),0);assert.equal(stepDecimals(0.25),2);assert.equal(stepDecimals(1e-7),7);
  assert.equal(snapValue(0.1+0.2,0,1,0.1),0.3);
  assert.equal(snapValue(7,0,5,1),5);assert.equal(snapValue(-3,0,5,1),0);
  assert.equal(snapValue(2.6,1,10,2),3,'the grid counts from min');
  assert.equal(snapValue(10,0,9.5,2),8,'never past max when max is off the grid');
  assert.equal(snapValue(Number.NaN,2,5,1),2);
  assert.equal(snapValue(0.123456,0,1),0.123456,'no step: only clamped');
  assert.equal(parseSliderText(' 0,5 '),0.5);assert.equal(parseSliderText('abc'),null);assert.equal(parseSliderText(''),null);
  assert.equal(formatSliderValue(0.5,0.01),'0.50');assert.equal(formatSliderValue(-0.0001,0.01),'0.00');assert.equal(formatSliderValue(3,1),'3');
});

test('a slider commits a drag once, when the pointer is released anywhere, and a key step at once',()=>{
  const run=(inputs)=>{let state=IDLE_GESTURE;const commits=[];for(const input of inputs){const next=sliderGesture(state,input);state=next.state;if(next.commit!==null)commits.push(next.commit);}return {state,commits};};
  // A drag: changes are drafted, the last one is committed on release (the range captures the pointer, so a release outside counts).
  assert.deepEqual(run([{kind:'down'},{kind:'change',value:1},{kind:'change',value:2},{kind:'up'}]),{state:IDLE_GESTURE,commits:[2]});
  // Release, cancel and lost capture may all arrive: only the first commits.
  assert.deepEqual(run([{kind:'down'},{kind:'change',value:3},{kind:'up'},{kind:'up'},{kind:'blur'}]).commits,[3]);
  // A click without movement commits nothing.
  assert.deepEqual(run([{kind:'down'},{kind:'up'}]).commits,[]);
  // Keys (no drag): each step commits at once.
  assert.deepEqual(run([{kind:'change',value:4},{kind:'change',value:5}]).commits,[4,5]);
  // Focus leaving in the middle of a drag commits what was drafted.
  assert.deepEqual(run([{kind:'down'},{kind:'change',value:6},{kind:'blur'}]).commits,[6]);
});
test('the command-line stand-in hashes files and lists derived items of stored plugin entries',async()=>{
  const root=tempDir('derived');
  writeFileSync(path.join(root,'a.bin'),'hello');
  assert.equal(await createFs().hashFile(root,'a.bin'),createHash('sha256').update('hello').digest('hex'));
  await assert.rejects(createFs().hashFile(root,'../outside'),/escapes/);
  assert.equal(await createFs().fileSize(root,'a.bin'),5);
  await assert.rejects(createFs().fileSize(root,'../outside'),/escapes/);
  const tbx=await loadPlugins([],{libraryRoot:root,store:true});
  writeFileSync(path.join(root,'a.bin'),'hello');
  const stage=(rel,content)=>{const target=path.join(root,'library',rel);mkdirSync(path.dirname(target),{recursive:true});writeFileSync(target,content);};
  stage('pack/substance/Tiles/tiles.sbsar','archive');stage('pack/substance/Tiles/preview.png','png');stage('pack/materials/tiles/albedo.png','map');
  await tbx.commit('com.toybox.material-sbsar',[
    {kind:'plugin',type:'substance',id:'uid://substance_1',pack:'pack',category:'substance',name:'Tiles',format:'sbsar',files:['tiles.sbsar','preview.png'],meta:{sbsar:'tiles.sbsar'}},
    {kind:'material',id:'uid://sbsar_a',pack:'pack',category:'substance',name:'tiles',format:'sbsar',maps:[{role:'albedo',file:'albedo.png'}],origin:{derivedFrom:'uid://substance_1',meta:{graph:0}}},
  ]);
  const substance=tbx.library.get('uid://substance_1');
  assert.deepEqual(substance.files,['library/substance/uidsubstance1/tiles.sbsar','library/substance/uidsubstance1/preview.png']);
  assert.equal(readFileSync(path.join(root,substance.files[0]),'utf8'),'archive');
  assert.equal(existsSync(path.join(root,'library/pack/substance/Tiles')),false,'the staged folder is moved');
  assert.equal(tbx.library.get('uid://sbsar_a').format,'sbsar');
  assert.deepEqual((await tbx.host.getDerivedAssets('uid://substance_1')).map(a=>a.id),['uid://sbsar_a']);
  assert.deepEqual(await tbx.host.getDerivedAssets('uid://sbsar_a'),[]);
  substance.user.tags=['kept'];
  stage('pack/substance/Tiles/tiles.sbsar','archive 2');
  await tbx.commit('com.toybox.material-sbsar',[{kind:'plugin',type:'substance',id:'uid://substance_1',pack:'pack',category:'substance',name:'Tiles',files:['tiles.sbsar'],meta:{}}]);
  assert.deepEqual(tbx.library.get('uid://substance_1').user.tags,['kept'],'a replaced item keeps its user data');
  assert.deepEqual(tbx.library.get('uid://substance_1').files,['library/substance/uidsubstance1/tiles.sbsar']);
  assert.equal(existsSync(path.join(root,'library/substance/uidsubstance1/preview.png')),false,'the old folder goes');
  await tbx.host.ensureAssetFiles(['uid://substance_1']);
});
