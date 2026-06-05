/**
 * The embedded Toybox demo. This is the real app's browsing surface — the same
 * FacetFilter, AssetGrid and 3D AssetViewer components, driven by the real
 * Zustand store and the real glTF loader / thumbnail renderer. Only the Tauri
 * seams are swapped (see src/demo/shims): asset URLs point at bundled files,
 * and the Rust command surface is replaced with browser-native no-ops.
 *
 * It mirrors LibraryView's composition, minus the file-dialog export flow that
 * has no meaning in a browser — the Export button opens a short explainer
 * instead of an OS save dialog.
 */

import { useEffect, useMemo, useState } from "react";

import { applyFilter } from "@app/domain/facets";
import { useStore } from "@app/state/store";
import { AssetGrid } from "@app/components/AssetGrid";
import { AssetViewer } from "@app/components/AssetViewer";
import { FacetFilter } from "@app/components/FacetFilter";
import { LibraryToolbar } from "@app/components/LibraryToolbar";
import { useThumbGeneration } from "@app/components/useThumbGeneration";
import { useFavorites } from "@app/components/useFavorites";
import { Button } from "@app/ds/Button";
import { Stack } from "@app/ds/Stack";

import { demoCatalog, DEMO_PACKS } from "./catalog";
import "./demo-app.css";

export default function ToyboxDemo() {
  const {
    catalog,
    packs: packMeta,
    filter,
    selection,
    previewId,
    setCatalog,
    setPacks,
    setText,
    togglePack,
    toggleCategory,
    toggleFavoritesOnly,
    clearFilter,
    toggleSelected,
    clearSelection,
    setPreview,
  } = useStore();

  // Seed the store from the fixture once, then start the real thumbnail queue
  // so cards fill in with genuine, live-rendered previews.
  useEffect(() => {
    setPacks(DEMO_PACKS);
    setCatalog(demoCatalog());
  }, [setPacks, setCatalog]);

  const assets = catalog?.assets ?? [];
  const visible = useMemo(() => applyFilter(assets, filter), [assets, filter]);
  const previewAsset = previewId ? assets.find((a) => a.id === previewId) : undefined;

  const { start: startThumbs } = useThumbGeneration(assets);
  useEffect(() => {
    if (assets.length > 0) void startThumbs();
  }, [assets.length, startThumbs]);

  const { toggleFavorite } = useFavorites();
  const [exportNote, setExportNote] = useState(false);

  return (
    <div className="toybox-app">
      <Stack dir="row" grow>
        <FacetFilter
          assets={assets}
          filter={filter}
          packMeta={packMeta}
          onTogglePack={togglePack}
          onToggleCategory={toggleCategory}
          onToggleFavoritesOnly={toggleFavoritesOnly}
          onClear={clearFilter}
        />

        <Stack grow>
          <LibraryToolbar
            searchText={filter.text}
            onSearchChange={setText}
            visibleCount={visible.length}
            totalCount={assets.length}
            selectedCount={selection.size}
            onClearSelection={clearSelection}
            onExport={() => setExportNote(true)}
          />

          <AssetGrid
            assets={visible}
            selection={selection}
            onToggleSelect={toggleSelected}
            onToggleFavorite={toggleFavorite}
            onOpen={setPreview}
          />
        </Stack>
      </Stack>

      {previewAsset && (
        <AssetViewer asset={previewAsset} onClose={() => setPreview(null)} />
      )}

      {exportNote && <ExportNote onClose={() => setExportNote(false)} />}
    </div>
  );
}

/**
 * Stand-in for the OS export dialog. In the app, Export writes a self-contained
 * copy or a merged .glb to a folder you pick; in the browser there's nothing to
 * write to, so we explain what the real button does instead.
 */
function ExportNote({ onClose }: { onClose: () => void }) {
  return (
    <div className="viewer" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="viewer__panel"
        style={{ maxWidth: 460, height: "auto", padding: "var(--space-5)" }}
      >
        <h3 style={{ margin: "0 0 var(--space-3)", color: "var(--text-0)" }}>
          Export
        </h3>
        <p style={{ margin: 0, color: "var(--text-1)", lineHeight: 1.6 }}>
          In the desktop app, Export writes your selected assets to a folder you
          choose — either a self-contained copy of the loose glTF files, or a
          single merged <code>.glb</code> assembled directly in Rust. The web
          demo has no filesystem to write to, so this button is inert here.
        </p>
        <div style={{ marginTop: "var(--space-4)", textAlign: "right" }}>
          <Button variant="primary" onClick={onClose}>
            Got it
          </Button>
        </div>
      </div>
    </div>
  );
}
