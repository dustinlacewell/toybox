import { Copy, Download, FolderInput, FolderOpen, FolderPlus, Maximize2, Tag, Trash2, X, type LucideIcon } from "lucide-react";
import { ContextMenu, type MenuEntry } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { Asset, LibraryInfo } from "../domain/catalog";
import { assetMenu, type AssetAction, type AssetMenuEntry } from "../domain/assetMenu";
import { deleteBlocker } from "../domain/transfer";
import { libraryOf } from "../domain/libraryId";

/** An open tile menu: where it opened, the tile right-clicked, and the ids it acts on. */
export interface AssetMenuOpen { x: number; y: number; asset: Asset; ids: string[] }

const ICONS: Record<AssetAction, LucideIcon> = {
  open: Maximize2, collect: FolderPlus, tag: Tag, copy: Copy, move: FolderInput,
  reveal: FolderOpen, export: Download, clear: X, delete: Trash2,
};

/** The context menu of a tile, acting on the tile or on the selection it belongs to. */
export function AssetContextMenu({ menu, libraries, selectionSize, offerTransfer, onAction, onClose }: {
  menu: AssetMenuOpen; libraries: readonly LibraryInfo[]; selectionSize: number; offerTransfer: boolean;
  onAction: (action: AssetAction, menu: AssetMenuOpen) => void; onClose: () => void;
}) {
  const { asset, ids } = menu;
  const single = ids.length === 1 ? { name: variantInfo(asset.name, asset.kind).base, remote: libraries.find((library) => library.id === asset.library)?.kind === "remote" } : undefined;
  const entries = assetMenu({
    target: { ids, single }, selectionSize, offerTransfer,
    sourceLocked: deleteBlocker(libraries, ids.map((id) => ({ library: libraryOf(id) }))) ? "read-only" : undefined,
  }).map(withIcon);
  return <ContextMenu key={`${menu.x},${menu.y}`} at={menu} entries={entries} label={entries[0].kind === "heading" ? entries[0].label : undefined}
    onPick={(id) => onAction(id as AssetAction, menu)} onClose={onClose} />;
}

function withIcon(entry: AssetMenuEntry): MenuEntry {
  return entry.kind === "item" ? { ...entry, icon: ICONS[entry.id] } : entry;
}
