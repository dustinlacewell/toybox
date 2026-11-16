import { IconButton } from "@ldlework/toybox-sdk/ui";
import { PanelLeftOpen, Star, Settings, Upload } from "lucide-react";
import type { AssetKind } from "../domain/catalog";
import { ASSET_KIND_LABELS } from "../domain/catalog";
import { AssetKindIcon } from "./AssetKindIcon";

export function CompactLibraryNavigation({kind,icons,favorites,onKind,onFavorites,onRestore,onSettings,onExports,narrow}:{
  kind:AssetKind;icons:Partial<Record<AssetKind,string>>;favorites:boolean;onKind:(kind:AssetKind)=>void;
  onFavorites:()=>void;onRestore:()=>void;onSettings:()=>void;onExports:()=>void;narrow:boolean;
}) {
  return <nav className="library-compact-navigation" aria-label="Library navigation">
    <IconButton icon={PanelLeftOpen} label={narrow?"Show navigation":"Restore sidebar"} onClick={onRestore}/>
    <span className="library-compact-navigation__separator"/>
    {(Object.keys(ASSET_KIND_LABELS) as AssetKind[]).map(value=><IconButton key={value} glyph={<AssetKindIcon kind={value} url={icons[value]}/>} label={ASSET_KIND_LABELS[value]} active={kind===value} aria-pressed={kind===value} onClick={()=>onKind(value)}/>)}
    <IconButton icon={Star} label="Favorites" active={favorites} aria-pressed={favorites} onClick={onFavorites}/>
    <span className="library-compact-navigation__spacer"/>
    <IconButton icon={Upload} label="Exports" onClick={onExports}/>
    <IconButton icon={Settings} label="Settings" onClick={onSettings}/>
  </nav>;
}
