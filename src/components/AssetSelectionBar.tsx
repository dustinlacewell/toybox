import { useEffect } from "react";
import { SelectionBar, type SelectionAction } from "@ldlework/toybox-sdk/ui";
import type { LibraryInfo } from "../domain/catalog";
import { selectionActions, type AssetAction } from "../domain/assetMenu";
import { deleteBlocker } from "../domain/transfer";
import { libraryOf } from "../domain/libraryId";
import { ASSET_ACTION_ICONS } from "./AssetContextMenu";

/**
 * The bar over the grid while several items are selected: the tile menu's actions for the selection, with Export as
 * the main one. Escape lets the selection go while nothing else (a dialog, a menu, a text field) has the key.
 */
export function AssetSelectionBar({ ids, libraries, offerTransfer, escapeClears, onAction, onClear }: {
  ids: readonly string[]; libraries: readonly LibraryInfo[]; offerTransfer: boolean; escapeClears: boolean;
  onAction: (action: AssetAction) => void; onClear: () => void;
}) {
  const sourceLocked = deleteBlocker(libraries, ids.map((id) => ({ library: libraryOf(id) }))) ? "read-only" : undefined;
  const actions: SelectionAction[] = selectionActions({ offerTransfer, sourceLocked })
    .map(({ id, label, tone, disabled, reason }) => ({ id, label, tone, disabled, reason, icon: ASSET_ACTION_ICONS[id], primary: id === "export" }));

  useEffect(() => {
    if (!escapeClears) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable='true'], [role='dialog']")) return;
      onClear();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [escapeClears, onClear]);

  return <SelectionBar label={`${ids.length.toLocaleString()} selected`} actions={actions} onAction={(id) => onAction(id as AssetAction)} onClear={onClear} />;
}
