import { PropertyRow, formatTime } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { Asset } from "../domain/catalog";
import { pluginProperties } from "../state/assetTypes";
import { useStore } from "../state/store";
import { revealAsset } from "../services/reveal";
import { RigFacts } from "./viewer/RigFacts";
import { importedFrom, provenanceValue } from "../domain/facets";
import { channelsLabel, clipLengthLabel, sampleRateLabel } from "../domain/assetCard";
import { AssetTags } from "./AssetTags";
import { AssetLibraryNote } from "./AssetLibraryNote";
import "./AssetProperties.css";

/** The property list of the full view's Details tab. */
export function AssetProperties({asset,onOpen,onCollect,showFileAction=true}:{asset:Asset;onOpen?:()=>void;onCollect?:()=>void;showFileAction?:boolean}) {
  const libraries=useStore(state=>state.libraries);
  const owner=libraries.find(library=>library.id===asset.library);
  const variants=[asset,...(asset.variants??[])];
  const imported=asset.kind==="model"||asset.kind==="animation"?importedFrom(asset):null;
  const source=provenanceValue(asset,"source"),publisher=provenanceValue(asset,"publisher"),license=provenanceValue(asset,"license");
  return <div className="asset-properties">
    {asset.kind === "model" ? <><RigFacts asset={asset} /><PropertyRow label="Detail levels" value={variants.length} onPress={onOpen} /></>
      : asset.kind === "material" ? <><PropertyRow label="Maps" value={asset.material.maps.length} onPress={onOpen} /><PropertyRow label="Sizes" value={variants.length} onPress={onOpen} /><PropertyRow label="Largest size" value={variantInfo(asset.name, "material").label} /></>
      : asset.kind === "audio" ? <><PropertyRow label="Length" value={formatTime(asset.audio.duration)} onPress={onOpen}/><PropertyRow label="Format" value={asset.audio.format}/><PropertyRow label="Channels" value={channelsLabel(asset.audio.channels)}/><PropertyRow label="Sample rate" value={sampleRateLabel(asset.audio.sampleRate)}/></>
      : asset.kind === "plugin" ? <>{pluginProperties(asset).map(({label,value})=><PropertyRow key={label} label={label} value={value}/>)}<PropertyRow label="Files" value={asset.files.length} /></>
      : <><PropertyRow label="Length" value={clipLengthLabel(asset.clip.duration)} onPress={onOpen} /><PropertyRow label="Rig" value={asset.clip.rig} /><PropertyRow label="Animation source" value={asset.clip.sourceName} onPress={onOpen} /><PropertyRow label="Animated nodes" value={asset.clip.targetNodes.length} /></>}
    <PropertyRow label="Category" value={asset.category} />
    {libraries.length>1&&<PropertyRow label="Library" value={<AssetLibraryNote asset={asset}/>}/>}
    {imported&&<PropertyRow label="Imported from" value={imported}/>}
    {source&&<PropertyRow label="Source" value={source}/>}
    {publisher&&<PropertyRow label="Publisher" value={publisher}/>}
    {license&&<PropertyRow label="License" value={license}/>}
    {asset.user.provenance&&<>
      {asset.user.provenance.url&&<PropertyRow label="Source URL" value={asset.user.provenance.url}/>}
    </>}
    <section className="asset-properties__tags"><div>Tags</div><AssetTags asset={asset} library={owner}/></section>
    {!!asset.user.collections?.length&&<PropertyRow label="Collections" value={asset.user.collections.join(", ")} onPress={onCollect}/>}
    {showFileAction&&owner?.kind!=="remote"&&<PropertyRow label="Source files" value="Show in folder" onPress={()=>void revealAsset(asset)}/>}
  </div>;
}
