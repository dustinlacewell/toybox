import { PropertyRow, Tag, formatTime } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { Asset } from "../domain/catalog";
import { revealAsset } from "../services/reveal";
import { RigFacts } from "./viewer/RigFacts";
import { isProvenanceTag, provenanceValue } from "../domain/facets";
import "./AssetProperties.css";

/** One property view shared by the library inspector and full-view Details tab. */
export function AssetProperties({asset,onOpen,onCollect,showFileAction=true}:{asset:Asset;onOpen?:()=>void;onCollect?:()=>void;showFileAction?:boolean}) {
  const variants=[asset,...(asset.variants??[])];
  const tags=[...new Set(asset.user.tags.filter(tag=>!isProvenanceTag(tag)))];
  const source=provenanceValue(asset,"source"),publisher=provenanceValue(asset,"publisher"),license=provenanceValue(asset,"license");
  return <div className="asset-properties">
    {asset.kind === "model" ? <><RigFacts asset={asset} /><PropertyRow label="Detail levels" value={variants.length} onPress={onOpen} /></>
      : asset.kind === "material" ? <><PropertyRow label="Maps" value={asset.material.maps.length} onPress={onOpen} /><PropertyRow label="Sizes" value={variants.length} onPress={onOpen} /><PropertyRow label="Largest size" value={variantInfo(asset.name, "material").label} /></>
      : asset.kind === "audio" ? <><PropertyRow label="Length" value={formatTime(asset.audio.duration)} onPress={onOpen}/><PropertyRow label="Format" value={asset.audio.format}/><PropertyRow label="Channels" value={asset.audio.channels===1?"Mono":asset.audio.channels===2?"Stereo":asset.audio.channels}/><PropertyRow label="Sample rate" value={`${asset.audio.sampleRate/1000} kHz`}/></>
      : <><PropertyRow label="Length" value={`${asset.clip.duration.toFixed(2)} s`} onPress={onOpen} /><PropertyRow label="Rig" value={asset.clip.rig} /><PropertyRow label="Animation source" value={asset.clip.sourceName} onPress={onOpen} /><PropertyRow label="Animated nodes" value={asset.clip.targetNodes.length} /></>}
    <PropertyRow label="Category" value={asset.category} />
    {source&&<PropertyRow label="Source" value={source}/>}
    {publisher&&<PropertyRow label="Publisher" value={publisher}/>}
    {license&&<PropertyRow label="License" value={license}/>}
    {asset.user.provenance&&<>
      {asset.user.provenance.url&&<PropertyRow label="Source URL" value={asset.user.provenance.url}/>}
    </>}
    {!!tags.length&&<section className="asset-properties__tags"><div>Tags</div><div className="asset-properties__tag-list" role="list" aria-label="Tags">{tags.map(tag=><Tag key={tag} role="listitem">{tag}</Tag>)}</div></section>}
    {!!asset.user.collections?.length&&<PropertyRow label="Collections" value={asset.user.collections.join(", ")} onPress={onCollect}/>}
    {showFileAction&&<PropertyRow label="Source files" value="Show in folder" onPress={()=>void revealAsset(asset)}/>}
  </div>;
}
