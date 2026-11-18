import { IconButton } from "@ldlework/toybox-sdk/ui";
import { PanelLeftOpen, Star, Settings, Upload } from "lucide-react";
import type { AssetKind } from "../domain/catalog";
import type { AssetTypeEntry } from "../services/assetTypes";
import { AssetKindIcon } from "./AssetKindIcon";

export function CompactLibraryNavigation({kind,types,icons,favorites,onKind,onFavorites,onRestore,onSettings,onExports,narrow}:{
  kind:AssetKind;types:AssetTypeEntry[];icons:Partial<Record<AssetKind,string>>;favorites:boolean;onKind:(kind:AssetKind)=>void;
  onFavorites:()=>void;onRestore:()=>void;onSettings:()=>void;onExports:()=>void;narrow:boolean;
}) {
  return <nav className="library-compact-navigation" aria-label="Library navigation">
    <IconButton icon={PanelLeftOpen} label={narrow?"Show navigation":"Restore sidebar"} onClick={onRestore}/>
    <span className="library-compact-navigation__separator"/>
    {types.map(({id,label})=><IconButton key={id} glyph={<AssetKindIcon kind={id} url={icons[id]}/>} label={label} active={kind===id} aria-pressed={kind===id} onClick={()=>onKind(id)}/>)}
    <IconButton icon={Star} label="Favorites" active={favorites} aria-pressed={favorites} onClick={onFavorites}/>
    <span className="library-compact-navigation__spacer"/>
    <IconButton icon={Upload} label="Exports" onClick={onExports}/>
    <IconButton icon={Settings} label="Settings" onClick={onSettings}/>
  </nav>;
}
