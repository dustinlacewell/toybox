import { PropertyRow, formatTime } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { AssetProperty } from "@ldlework/toybox-sdk";
import type { Asset, PluginAsset } from "../domain/catalog";
import { assetTypeEntry } from "../state/assetTypes";
import { useStore } from "../state/store";
import { revealAsset } from "../services/reveal";
import { RigFacts } from "./viewer/RigFacts";
import { RemoteMark } from "./RemoteMark";
import { useAlsoIn, useOtherVersions } from "../state/duplicates";
import { importedFrom, provenanceValue } from "../domain/facets";
import { AssetTags } from "./AssetTags";
import "./AssetProperties.css";

/** A plugin type's own rows. A throwing or malformed plugin contributes none. */
function pluginProperties(asset:PluginAsset):AssetProperty[] {
  try {
    const rows=assetTypeEntry(asset.type).implementation?.properties?.(asset)??[];
    return Array.isArray(rows)?rows.filter(row=>row&&typeof row.label==="string"&&(typeof row.value==="string"||typeof row.value==="number")):[];
  } catch { return []; }
}

/** One property view shared by the library inspector and full-view Details tab. */
export function AssetProperties({asset,onOpen,onCollect,showFileAction=true}:{asset:Asset;onOpen?:()=>void;onCollect?:()=>void;showFileAction?:boolean}) {
  const libraries=useStore(state=>state.libraries);
  const owner=libraries.find(library=>library.id===asset.library);
  const elsewhere=useAlsoIn(asset).map(copy=>libraries.find(library=>library.id===copy.library)).filter(library=>!!library);
  const different=useOtherVersions(asset).map(copy=>libraries.find(library=>library.id===copy.library)).filter(library=>!!library);
  const variants=[asset,...(asset.variants??[])];
  const imported=asset.kind==="model"||asset.kind==="animation"?importedFrom(asset):null;
  const source=provenanceValue(asset,"source"),publisher=provenanceValue(asset,"publisher"),license=provenanceValue(asset,"license");
  return <div className="asset-properties">
    {asset.kind === "model" ? <><RigFacts asset={asset} /><PropertyRow label="Detail levels" value={variants.length} onPress={onOpen} /></>
      : asset.kind === "material" ? <><PropertyRow label="Maps" value={asset.material.maps.length} onPress={onOpen} /><PropertyRow label="Sizes" value={variants.length} onPress={onOpen} /><PropertyRow label="Largest size" value={variantInfo(asset.name, "material").label} /></>
      : asset.kind === "audio" ? <><PropertyRow label="Length" value={formatTime(asset.audio.duration)} onPress={onOpen}/><PropertyRow label="Format" value={asset.audio.format}/><PropertyRow label="Channels" value={asset.audio.channels===1?"Mono":asset.audio.channels===2?"Stereo":asset.audio.channels}/><PropertyRow label="Sample rate" value={`${asset.audio.sampleRate/1000} kHz`}/></>
      : asset.kind === "plugin" ? <>{pluginProperties(asset).map(({label,value})=><PropertyRow key={label} label={label} value={value}/>)}<PropertyRow label="Files" value={asset.files.length} /></>
      : <><PropertyRow label="Length" value={`${asset.clip.duration.toFixed(2)} s`} onPress={onOpen} /><PropertyRow label="Rig" value={asset.clip.rig} /><PropertyRow label="Animation source" value={asset.clip.sourceName} onPress={onOpen} /><PropertyRow label="Animated nodes" value={asset.clip.targetNodes.length} /></>}
    <PropertyRow label="Category" value={asset.category} />
    {libraries.length>1&&<PropertyRow label="Library" value={<>{owner?.name??asset.library}<RemoteMark library={owner}/>{!!elsewhere.length&&<> · also in {elsewhere.map((library,index)=><span key={library!.id}>{index>0&&", "}{library!.name}<RemoteMark library={library}/></span>)}</>}{!!different.length&&<> · a different version is in {different.map((library,index)=><span key={library!.id}>{index>0&&", "}{library!.name}<RemoteMark library={library}/></span>)}</>}</>}/>}
    {imported&&<PropertyRow label="Imported from" value={imported}/>}
    {source&&<PropertyRow label="Source" value={source}/>}
    {publisher&&<PropertyRow label="Publisher" value={publisher}/>}
    {license&&<PropertyRow label="License" value={license}/>}
    {asset.user.provenance&&<>
      {asset.user.provenance.url&&<PropertyRow label="Source URL" value={asset.user.provenance.url}/>}
    </>}
    <AssetTags asset={asset} library={owner}/>
    {!!asset.user.collections?.length&&<PropertyRow label="Collections" value={asset.user.collections.join(", ")} onPress={onCollect}/>}
    {showFileAction&&owner?.kind!=="remote"&&<PropertyRow label="Source files" value="Show in folder" onPress={()=>void revealAsset(asset)}/>}
  </div>;
}
