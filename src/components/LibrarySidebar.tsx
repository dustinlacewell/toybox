import { IconButton, NavigationGroup, NavigationItem } from "@ldlework/toybox-sdk/ui";
import { Settings, FolderSync, PanelLeftClose, Upload } from "lucide-react";
import { AssetKindIcon } from "./AssetKindIcon";
import type { Asset, AssetKind } from "../domain/catalog";
import type { Filter } from "../domain/facets";
import { ofKind } from "../domain/facets";
import type { ReactNode } from "react";

interface Props {
  assets: Asset[]; filter: Filter;
  collections: string[]; collection: string | null; onCollection: (name: string | null) => void;
  onKind: (kind: AssetKind) => void;
  onExports: () => void; onSettings: () => void; onRescan: () => void;
  filters:ReactNode;
  onMaximize?:()=>void;
  icons:Partial<Record<AssetKind,string>>;
}
export function LibrarySidebar({ assets, filter, collections, collection, onCollection, onKind, onExports, onSettings, onRescan, filters, onMaximize, icons }: Props) {
  const current = ofKind(assets, filter.kind);
  return <>
    <div className="library-sidebar__header"><div className="library-brand"><span>Toybox</span>{onMaximize&&<IconButton icon={PanelLeftClose} label="Maximize library" onClick={onMaximize}/>}</div>
      <div className="library-sidebar__utilities" role="toolbar" aria-label="Library actions"><IconButton icon={Settings} label="Settings" onClick={onSettings}/><IconButton icon={FolderSync} label="Rescan library" onClick={onRescan}/><IconButton icon={Upload} label="Exports" onClick={onExports}/></div>
    </div>
    {([['model', 'Models'], ['material', 'Materials'], ['animation', 'Animations'], ['audio', 'Audio']] as const).map(([kind, label]) => <NavigationItem key={kind} label={<span className="library-type-label"><AssetKindIcon kind={kind} url={icons[kind]}/>{label}</span>} detail={ofKind(assets, kind).length.toLocaleString()} active={filter.kind === kind && !collection && !filter.favoritesOnly && !filter.sources.size} onPress={() => { onKind(kind); onCollection(null); }} />)}
    {!!collections.length&&<NavigationGroup label="Collections">
      {collections.map((name) => <NavigationItem key={name} label={name} detail={current.filter((a) => a.user.collections?.includes(name)).length} active={collection === name} onPress={() => onCollection(collection === name ? null : name)} />)}
    </NavigationGroup>}
    {filters}
  </>;
}
