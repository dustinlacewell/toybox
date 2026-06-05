/**
 * The browsing toolbar shared by the desktop app and the web demo: search box,
 * visible/total + selection counts, and the action icon strip. Fully
 * controlled — every value and action is a prop, so each surface supplies its
 * own callbacks (the demo omits the library-only ones and points Export at an
 * explainer instead of the OS dialog).
 */

import type { ReactNode } from "react";
import { Download, RefreshCw, ScanLine, X } from "lucide-react";

import { Toolbar } from "../ds/Toolbar";
import { TextInput } from "../ds/TextInput";
import { IconButton } from "../ds/IconButton";
import { IconStrip } from "../ds/IconStrip";

interface Props {
  searchText: string;
  onSearchChange: (text: string) => void;
  visibleCount: number;
  totalCount: number;
  selectedCount: number;
  onClearSelection: () => void;
  onExport: () => void;
  /** Library-only: rebuild the catalog. Omitted by the web demo. */
  onRescan?: () => void;
  /** Library-only: regenerate every thumbnail. Omitted by the web demo. */
  onRegenerate?: () => void;
  /** The thumbnail-generation control/progress, when the surface has one. */
  thumbControl?: ReactNode;
}

export function LibraryToolbar({
  searchText,
  onSearchChange,
  visibleCount,
  totalCount,
  selectedCount,
  onClearSelection,
  onExport,
  onRescan,
  onRegenerate,
  thumbControl,
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
      {thumbControl}
      <span className="lib__count">{selectedCount} selected</span>
      <IconStrip>
        {onRegenerate && (
          <IconButton icon={RefreshCw} label="Regenerate thumbnails" onClick={onRegenerate} />
        )}
        {onRescan && <IconButton icon={ScanLine} label="Rescan library" onClick={onRescan} />}
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
