import { IconButton, NavigationGroup, NavigationItem } from "@ldlework/toybox-sdk/ui";
import { Settings, PanelLeftClose, Upload } from "lucide-react";
import { AssetKindIcon } from "./AssetKindIcon";
import type { AssetKind } from "../domain/catalog";
import type { Filter } from "../domain/facets";
import type { ReactNode } from "react";
import type { AssetTypeEntry } from "../services/assetTypes";

interface Props {
  /** Items per type, each once (nothing until the libraries have answered). */
  typeCounts?: ReadonlyMap<string, number>; filter: Filter;
  /** The collections, with how many items of the type being shown each holds. */
  collections: { name: string; count: number }[]; collection: string | null; onCollection: (name: string | null) => void;
  onKind: (kind: AssetKind) => void;
  onExports: () => void; onSettings: () => void;
  filters:ReactNode;
  onMaximize?:()=>void;
  icons:Partial<Record<AssetKind,string>>;
  /** The library's sections, in navigation order. */
  types:AssetTypeEntry[];
}
export function LibrarySidebar({ typeCounts, filter, collections, collection, onCollection, onKind, onExports, onSettings, filters, onMaximize, icons, types }: Props) {
  return <>
    <div className="library-sidebar__header"><div className="library-brand"><span>Toybox</span>{onMaximize&&<IconButton icon={PanelLeftClose} label="Maximize library" onClick={onMaximize}/>}</div>
      <div className="library-sidebar__utilities" role="toolbar" aria-label="Library actions"><IconButton icon={Settings} label="Settings" onClick={onSettings}/><IconButton icon={Upload} label="Exports" onClick={onExports}/></div>
    </div>
    {types.map(({id:kind,label}) => <NavigationItem key={kind} label={<span className="library-type-label"><AssetKindIcon kind={kind} url={icons[kind]}/>{label}</span>} detail={(typeCounts?.get(kind) ?? 0).toLocaleString()} active={filter.kind === kind && !collection && !filter.favoritesOnly && !filter.sources.size} onPress={() => { onKind(kind); onCollection(null); }} />)}
    {!!collections.length&&<NavigationGroup label="Collections">
      {collections.map(({ name, count }) => <NavigationItem key={name} label={name} detail={count} active={collection === name} onPress={() => onCollection(collection === name ? null : name)} />)}
    </NavigationGroup>}
    {filters}
  </>;
}
