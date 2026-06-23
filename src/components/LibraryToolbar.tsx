/**
 * The browsing toolbar shared by the desktop app and the web demo: search box,
 * visible/total + selection counts, and the action icon strip. Fully
 * controlled — every value and action is a prop, so each surface supplies its
 * own callbacks (the demo omits the library-only ones and points Export at an
 * explainer instead of the OS dialog).
 */

import { Download, RefreshCw, ScanLine, Settings, Upload, X } from "lucide-react";

import { Toolbar, TextInput, IconButton, IconStrip } from "@ldlework/toybox-sdk/ui";

interface Props {
  searchText: string;
  onSearchChange: (text: string) => void;
  visibleCount: number;
  totalCount: number;
  selectedCount: number;
  onClearSelection: () => void;
  onExport: () => void;
  /** Library-only: open the import drawer (plugin-driven ingestion). */
  onImport?: () => void;
  /** Library-only: rebuild the catalog. Omitted by the web demo. */
  onRescan?: () => void;
  /** Library-only: open the settings drawer (library + FBX converter). */
  onOpenSettings?: () => void;
  /** Library-only: regenerate every thumbnail. Omitted by the web demo. */
  onRegenerate?: () => void;
}

export function LibraryToolbar({
  searchText,
  onSearchChange,
  visibleCount,
  totalCount,
  selectedCount,
  onClearSelection,
  onExport,
  onImport,
  onRescan,
  onOpenSettings,
  onRegenerate,
}: Props) {
  const hasSelection = selectedCount > 0;
  return (
    <Toolbar>
      <TextInput
        placeholder="Search assets…"
        value={searchText}
        onChange={(e) => onSearchChange(e.currentTarget.value)}
        style={{ maxWidth: 320 }}
      />
      <span className="lib__count">
        {visibleCount} / {totalCount}
      </span>
      <div style={{ flex: 1 }} />
      <span className="lib__count">{selectedCount} selected</span>
      <IconStrip>
        {onImport && <IconButton icon={Upload} label="Import assets…" onClick={onImport} />}
        {onRegenerate && (
          <IconButton icon={RefreshCw} label="Regenerate thumbnails" onClick={onRegenerate} />
        )}
        {onRescan && <IconButton icon={ScanLine} label="Rescan library" onClick={onRescan} />}
        {onOpenSettings && (
          <IconButton icon={Settings} label="Settings…" onClick={onOpenSettings} />
        )}
        <IconButton
          icon={X}
          label="Clear selection"
          onClick={onClearSelection}
          disabled={!hasSelection}
        />
        <IconButton
          icon={Download}
          label="Export selection"
          onClick={onExport}
          disabled={!hasSelection}
        />
      </IconStrip>
    </Toolbar>
  );
}
