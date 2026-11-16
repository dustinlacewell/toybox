import { Box3, Group, Mesh, MeshStandardMaterial } from "three";
import type { MaterialAsset, ModelAsset } from "../domain/catalog";
import { loadAssetScene } from "./gltfLoad";
import { loadMaterial } from "./materialLoad";
import { buildPreviewMesh } from "./materialPreview";
import { disposeScene } from "./disposeScene";
import { studioEnvironment } from "./studioEnvironment";
import type { HoverPreviewSource, LiveScene } from "./hoverScenePreview";

export function staticPreviewSource(asset:ModelAsset|MaterialAsset,revision:readonly unknown[],shape:"sphere"|"cube"="sphere"):HoverPreviewSource {
  return {key:asset.id,revision,load:async signal=>{
    signal.throwIfAborted();let content:Group;
    if(asset.kind==="model")content=(await loadAssetScene(asset,{signal})).scene;
    else {
      const material=await loadMaterial(asset,{maxSize:1024,maxTextureSize:1024,signal});
      content=new Group();content.add(buildPreviewMesh(material,shape));
    }
    try {signal.throwIfAborted();return createStaticScene(content);}
    catch(error){disposeScene(content);throw error;}
  }};
}

/** The host orbits a fixed-up camera around these bounds, including offset origins. */
export function createStaticScene(content:Group):LiveScene {
  content.updateWorldMatrix(true,true);const bounds=new Box3().setFromObject(content);
  if(bounds.isEmpty())throw Error("No visible geometry to preview");
  const subject=content;
  return {subject,bounds,continuous:false,reset:()=>{},tick:()=>{},
    prepare:({renderer})=>{const env=studioEnvironment(renderer);content.traverse(node=>{
      if(!(node instanceof Mesh))return;for(const material of Array.isArray(node.material)?node.material:[node.material]) {
        if(material instanceof MeshStandardMaterial&&material.envMap!==env){material.envMap=env;material.needsUpdate=true;}
      }
    });},dispose:()=>disposeScene(subject)};
}
