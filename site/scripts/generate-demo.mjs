import {mkdirSync,writeFileSync,readFileSync} from 'node:fs';
import {dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {deflateSync} from 'node:zlib';
import {BoxGeometry,Matrix4,Quaternion,Vector3} from 'three';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const output=resolve(root,'public/demo-assets/samples');
const prefix='toybox_samples/samples';
const assets=[];
const user={favorite:false,tags:['source: Toybox samples','publisher: Toybox'],provenance:{source:'Toybox samples',publisher:'Toybox'}};
const base=(id,name,kind,category,file)=>({id,name,kind,category,fileName:file,pack:'toybox_samples',relPath:`${prefix}/${file}`,thumb:{state:'missing'},user});
function write(file,bytes){const path=resolve(output,file);mkdirSync(dirname(path),{recursive:true});writeFileSync(path,bytes);}
const json=(file,value)=>write(file,JSON.stringify(value));
function crc(bytes){let value=0xffffffff;for(const byte of bytes){value^=byte;for(let bit=0;bit<8;bit++)value=(value>>>1)^((value&1)?0xedb88320:0);}return (value^0xffffffff)>>>0;}
function png(file,pixel){const size=128,data=Buffer.alloc((size*4+1)*size);for(let y=0;y<size;y++)for(let x=0;x<size;x++){const at=y*(size*4+1)+1+x*4;data.set([...pixel(x,y),255],at);}const chunk=(name,body)=>{const type=Buffer.from(name),length=Buffer.alloc(4),sum=Buffer.alloc(4);length.writeUInt32BE(body.length);sum.writeUInt32BE(crc(Buffer.concat([type,body])));return Buffer.concat([length,type,body,sum]);};const header=Buffer.alloc(13);header.writeUInt32BE(size,0);header.writeUInt32BE(size,4);header[8]=8;header[9]=6;write(file,Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(data)),chunk('IEND',Buffer.alloc(0))]));}
png('materials/Blue_tiles.png',(x,y)=>(x%32<2||y%32<2)?[18,29,45]:((x>>5)+(y>>5))%2?[44,102,165]:[62,129,191]);
png('materials/Terracotta.png',(x,y)=>(x%32<2||y%32<2)?[50,36,31]:((x>>5)+(y>>5))%2?[166,77,49]:[193,100,67]);
png('materials/Brushed_metal.png',(x,y)=>{const n=((y*17+x%3)%19);return [134+n,140+n,149+n];});
png('materials/Metalness.png',()=>[255,255,255]);
for(const [name,file,metal]of [['Blue tiles','Blue_tiles.png',false],['Terracotta','Terracotta.png',false],['Brushed metal','Brushed_metal.png',true]])assets.push({...base(`sample:material:${file}`,name,'material','surfaces',`materials/${file}`),material:{flipNormalGreen:false,maps:[{role:'albedo',file:`${prefix}/materials/${file}`},...(metal?[{role:'metallic',file:`${prefix}/materials/Metalness.png`}]:[])]}});

const geometry=new BoxGeometry(1,1,1),views=[],accessors=[],parts=[];let offset=0;
for(const [name,size,type]of [['position',3,5126],['normal',3,5126],['uv',2,5126],['index',1,5123]]){const attribute=name==='index'?geometry.index:geometry.attributes[name];const bytes=Buffer.from(attribute.array.buffer,attribute.array.byteOffset,attribute.array.byteLength);views.push({buffer:0,byteOffset:offset,byteLength:bytes.length});accessors.push({bufferView:views.length-1,componentType:type,count:attribute.count,type:size===1?'SCALAR':`VEC${size}`,...(name==='position'?{min:[-.5,-.5,-.5],max:[.5,.5,.5]}:{})});parts.push(bytes);offset+=bytes.length;}
const bytes=Buffer.concat(parts);write('models/Cube.bin',bytes);
const cube={asset:{version:'2.0'},scene:0,scenes:[{nodes:[0]}],nodes:[{name:'Cube',mesh:0,translation:[0,.5,0]}],meshes:[{primitives:[{attributes:{POSITION:0,NORMAL:1,TEXCOORD_0:2},indices:3,material:0}]}],materials:[{pbrMetallicRoughness:{baseColorFactor:[.12,.46,.8,1],metallicFactor:0,roughnessFactor:.45}}],buffers:[{byteLength:bytes.length,uri:`data:application/octet-stream;base64,${bytes.toString('base64')}`}],bufferViews:views,accessors};
json('models/Cube.gltf',cube);geometry.dispose();
const binding={rigId:'toybox-demo-cube',nodes:{Cube:0}};
assets.push({...base('sample:cube','Demo cube','model','props','models/Cube.gltf'),fileset:{gltf:`${prefix}/models/Cube.gltf`,bin:`${prefix}/models/Cube.bin`,textures:[]},animation:{clipCount:0,clipNames:[]},rigs:[binding]});
const times=new Float32Array([0,.5,1,1.5,2]),positions=new Float32Array([0,.5,0,0,1.2,0,0,.5,0,0,1.2,0,0,.5,0]);
const animationBytes=Buffer.concat([Buffer.from(times.buffer),Buffer.from(positions.buffer)]);write('animations/Bounce.bin',animationBytes);
json('animations/Bounce.gltf',{asset:{version:'2.0'},scene:0,scenes:[{nodes:[0]}],nodes:[{name:'Cube',translation:[0,.5,0]}],buffers:[{byteLength:animationBytes.length,uri:`data:application/octet-stream;base64,${animationBytes.toString('base64')}`}],bufferViews:[{buffer:0,byteOffset:0,byteLength:times.byteLength},{buffer:0,byteOffset:times.byteLength,byteLength:positions.byteLength}],accessors:[{bufferView:0,componentType:5126,count:5,type:'SCALAR',min:[0],max:[2]},{bufferView:1,componentType:5126,count:5,type:'VEC3'}],animations:[{name:'Bounce',channels:[{sampler:0,target:{node:0,path:'translation'}}],samplers:[{input:0,output:1,interpolation:'LINEAR'}]}]});
assets.push({...base('sample:bounce','Bounce','animation','motion','animations/Bounce.gltf'),fileset:{gltf:`${prefix}/animations/Bounce.gltf`,bin:`${prefix}/animations/Bounce.bin`,textures:[]},rigs:[binding],clip:{standalone:true,index:0,name:'Bounce',duration:2,sourceId:'sample:cube',sourceName:'Demo cube',targetNodes:['Cube'],rig:'Cube · 1 node',tracks:[{node:0,path:'translation'}]}});

for(const [name,duration,frequency]of [['UI blip',.22,880],['Pickup chime',.5,660],['Soft impact',.35,100]]){
 const rate=22050,count=Math.round(rate*duration),data=Buffer.alloc(44+count*2),peaks=Array(64).fill(0);data.write('RIFF');data.writeUInt32LE(data.length-8,4);data.write('WAVEfmt ',8);data.writeUInt32LE(16,16);data.writeUInt16LE(1,20);data.writeUInt16LE(1,22);data.writeUInt32LE(rate,24);data.writeUInt32LE(rate*2,28);data.writeUInt16LE(2,32);data.writeUInt16LE(16,34);data.write('data',36);data.writeUInt32LE(count*2,40);
 for(let i=0;i<count;i++){const t=i/rate,envelope=Math.min(1,t/.005)*Math.exp(-t/duration*5),sample=Math.sin(2*Math.PI*frequency*t)*envelope*.7;data.writeInt16LE(Math.round(sample*32767),44+i*2);const bucket=Math.min(63,Math.floor(i/count*64));peaks[bucket]=Math.max(peaks[bucket],Math.abs(sample));}
 const file=`audio/${name.replaceAll(' ','_')}.wav`;write(file,data);assets.push({...base(`sample:audio:${name}`,name,'audio','interface',file),audio:{file:`${prefix}/${file}`,format:'wav',duration,sampleRate:rate,channels:1,bitDepth:16,byteLength:data.length,modifiedNs:'0',peaks}});
}
const rigs=[{id:binding.rigId,name:'Cube',kind:'nodes',unambiguous:true,nodes:[{key:'Cube',name:'Cube',parent:null,rest:[1,0,0,0,0,1,0,0,0,0,1,0,0,.5,0,1]}]}];
// Use Toybox's bundled CC0 character with original, purpose-built demo motion.
const glb=readFileSync(resolve(root,'../public/preview/mannequin.glb'));
const jsonLength=glb.readUInt32LE(12),character=JSON.parse(glb.subarray(20,20+jsonLength).toString());
const binary=glb.subarray(28+jsonLength);
character.buffers[0].uri=`data:application/octet-stream;base64,${binary.toString('base64')}`;
json('models/Mannequin.gltf',character);write('models/Mannequin.bin',binary);
const characterBinding={rigId:'toybox-demo-character',nodes:Object.fromEntries(character.nodes.map((n,i)=>[n.name??`node${i}`,i]))};
const parent=new Map();character.nodes.forEach(n=>(n.children??[]).forEach(i=>parent.set(i,n.name)));
rigs.push({id:characterBinding.rigId,name:'Toybox mannequin',kind:'skeleton',unambiguous:true,nodes:character.nodes.map((n,i)=>({key:n.name??`node${i}`,name:n.name??`node${i}`,parent:parent.get(i)??null,rest:n.matrix??new Matrix4().compose(new Vector3(...(n.translation??[0,0,0])),new Quaternion(...(n.rotation??[0,0,0,1])),new Vector3(...(n.scale??[1,1,1]))).toArray()}))});
const characterUser={favorite:false,tags:['source: Universal Base Characters','publisher: Quaternius','license: CC0-1.0'],provenance:{source:'Universal Base Characters',publisher:'Quaternius',license:'CC0-1.0',url:'https://quaternius.itch.io/universal-base-characters'}};
assets.push({...base('sample:mannequin','Preview mannequin','model','characters','models/Mannequin.gltf'),user:characterUser,fileset:{gltf:`${prefix}/models/Mannequin.gltf`,bin:`${prefix}/models/Mannequin.bin`,textures:[]},animation:{clipCount:0,clipNames:[]},rigs:[characterBinding]});
for(const [name,bone,axis,angles]of [['Wave','upperarm_r',[1,0,0],[0,.8,0,.8,0]],['Look around','spine_03',[0,1,0],[0,.3,0,-.3,0]],['Stretch','spine_01',[1,0,0],[0,-.25,-.4,-.25,0]]]) {
 const node=character.nodes.findIndex(n=>n.name===bone),rest=new Quaternion(...(character.nodes[node].rotation??[0,0,0,1]));
 const rotations=new Float32Array(angles.flatMap(angle=>rest.clone().multiply(new Quaternion().setFromAxisAngle(new Vector3(...axis),angle)).toArray()));
 const data=Buffer.concat([Buffer.from(times.buffer),Buffer.from(rotations.buffer)]),file=name.replaceAll(' ','_');
 write(`animations/${file}.bin`,data);
 const nodes=character.nodes.map(({mesh,skin,...n})=>n);
 json(`animations/${file}.gltf`,{asset:{version:'2.0'},scene:character.scene,scenes:character.scenes,nodes,buffers:[{byteLength:data.length,uri:`data:application/octet-stream;base64,${data.toString('base64')}`}],bufferViews:[{buffer:0,byteOffset:0,byteLength:times.byteLength},{buffer:0,byteOffset:times.byteLength,byteLength:rotations.byteLength}],accessors:[{bufferView:0,componentType:5126,count:5,type:'SCALAR',min:[0],max:[2]},{bufferView:1,componentType:5126,count:5,type:'VEC4'}],animations:[{name,channels:[{sampler:0,target:{node,path:'rotation'}}],samplers:[{input:0,output:1,interpolation:'LINEAR'}]}]});
 assets.push({...base(`sample:animation:${file}`,name,'animation','motion',`animations/${file}.gltf`),fileset:{gltf:`${prefix}/animations/${file}.gltf`,bin:`${prefix}/animations/${file}.bin`,textures:[]},rigs:[characterBinding],clip:{standalone:true,index:0,name,duration:2,sourceId:'sample:mannequin',sourceName:'Preview mannequin',targetNodes:[bone],rig:'Toybox mannequin',tracks:[{node,path:'rotation'}]}});
}
writeFileSync(resolve(root,'src/demo/_samples.json'),JSON.stringify({assets,rigs},null,2)+'\n');
console.log(`Generated ${assets.length} self-contained demo samples.`);
