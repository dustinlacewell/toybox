import { IconButton, NavigationGroup, NavigationItem } from "@ldlework/toybox-sdk/ui";
import { Bookmark, Layers, Plus, Settings, PanelLeftClose, Upload, X } from "lucide-react";
import { AssetKindIcon } from "./AssetKindIcon";
import type { AssetKind } from "../domain/catalog";
import { ALL_TYPES, type Filter } from "../domain/facets";
import type { SavedView } from "../domain/savedViews";
import type { AssetTypeEntry } from "../services/assetTypes";

interface Props {
  /** Items per type, each once (nothing until the libraries have answered). */
  typeCounts?: ReadonlyMap<string, number>; filter: Filter;
  /** The collections, with how many items of the type being shown each holds. */
  collections: { name: string; count: number }[]; collection: string | null; onCollection: (name: string | null) => void;
  onKind: (kind: AssetKind) => void;
  onExports: () => void; onSettings: () => void;
  onMaximize?:()=>void;
  icons:Partial<Record<AssetKind,string>>;
  /** The library's sections, in navigation order. */
  types:AssetTypeEntry[];
  views: SavedView[]; activeView: string | null;
  onView: (view: SavedView) => void; onRemoveView: (view: SavedView) => void; onSaveView: () => void;
}
export function LibrarySidebar({ typeCounts, filter, collections, collection, onCollection, onKind, onExports, onSettings, onMaximize, icons, types, views, activeView, onView, onRemoveView, onSaveView }: Props) {
  const total = typeCounts ? [...typeCounts.values()].reduce((sum, count) => sum + count, 0) : 0;
  const plain = !collection && !activeView;
  return <>
    <div className="library-sidebar__header"><div className="library-brand"><span>Toybox</span>{onMaximize&&<IconButton icon={PanelLeftClose} label="Maximize library" onClick={onMaximize}/>}</div>
      <div className="library-sidebar__utilities" role="toolbar" aria-label="Library actions"><IconButton icon={Settings} label="Settings" onClick={onSettings}/><IconButton icon={Upload} label="Exports" onClick={onExports}/></div>
    </div>
    <NavigationItem label={<span className="library-type-label"><Layers size={18} className="asset-kind-icon" aria-hidden="true"/>All assets</span>} detail={total.toLocaleString()} active={filter.kind === ALL_TYPES && plain} onPress={() => { onKind(ALL_TYPES); onCollection(null); }} />
    {types.map(({id:kind,label}) => <NavigationItem key={kind} label={<span className="library-type-label"><AssetKindIcon kind={kind} url={icons[kind]}/>{label}</span>} detail={(typeCounts?.get(kind) ?? 0).toLocaleString()} active={filter.kind === kind && plain} onPress={() => { onKind(kind); onCollection(null); }} />)}
    {!!collections.length&&<NavigationGroup label="Collections">
      {collections.map(({ name, count }) => <NavigationItem key={name} label={name} detail={count} active={collection === name} onPress={() => onCollection(collection === name ? null : name)} />)}
    </NavigationGroup>}
    <NavigationGroup label="Saved views">
      {views.map((view) => <div key={view.id} className="library-view-item">
        <NavigationItem label={<span className="library-type-label"><Bookmark size={16} aria-hidden="true"/>{view.name}</span>} active={activeView === view.id} onPress={() => onView(view)} />
        <button type="button" className="library-view-item__remove" aria-label={`Remove the view ${view.name}`} title="Remove view" onClick={() => onRemoveView(view)}><X size={13}/></button>
      </div>)}
      <button type="button" className="library-view-save" onClick={onSaveView}><Plus size={14} aria-hidden="true"/>Save current view</button>
    </NavigationGroup>
  </>;
}
