import type { AssetKind } from "../domain/catalog";
import type { LoadedImporter } from "./pluginRegistry";
import { pluginUrl } from "./pluginLoader";

/** A stable importer choice supplies each built-in type's navigation artwork. */
export function pluginTypeIcons(importers:LoadedImporter[]):Partial<Record<AssetKind,string>> {
  const icons:Partial<Record<AssetKind,string>>={};
  for(const {manifest} of [...importers].sort((a,b)=>a.manifest.id.localeCompare(b.manifest.id))) {
    for(const kind of manifest.assetKinds??["model"]) {
      const path=manifest.assetIcons?.[kind]??manifest.icon;
      // Artwork stays inside its plugin, served through the existing jailed protocol.
      if(!icons[kind]&&typeof path==="string"&&path&&!path.includes(":")&&!path.includes("\\")&&!path.startsWith("/")&&!path.split("/").some(part=>part===".."||part==="."))icons[kind]=pluginUrl(manifest.id,path);
    }
  }
  return icons;
}
