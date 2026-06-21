/**
 * The main browsing view: facet rail + search/selection toolbar + virtualized
 * grid. Loads the catalog on mount (scanning to seed it the first time), and
 * derives the visible list with the pure facet selectors.
 */

import { useEffect, useMemo, useState } from "react";

import type { Catalog, PackMeta } from "../domain/catalog";
import { applyFilter } from "../domain/facets";
import { loadCatalog, loadPacks, scanLibrary } from "../services/tauriApi";
import { useStore } from "../state/store";
import { AssetGrid } from "../components/AssetGrid";
import { AssetViewer } from "../components/AssetViewer";
import { ExportDrawer } from "../components/ExportDrawer";
import { ImportDrawer } from "../components/ImportDrawer";
import { FacetFilter } from "../components/FacetFilter";
import { LibraryToolbar } from "../components/LibraryToolbar";
import { ThumbProgress } from "../components/ThumbProgress";
import { useThumbGeneration } from "../components/useThumbGeneration";
import { useFavorites } from "../components/useFavorites";
import { usePluginRegistry } from "../components/usePluginRegistry";
import { Button, Spinner, Stack } from "@ldlework/toybox-sdk/ui";
import "./LibraryView.css";

export function LibraryView() {
  const {
    catalog,
    packs: packMeta,
    loading,
    error,
    filter,
    selection,
    previewId,
    setCatalog,
    setPacks,
    setLoading,
    setError,
    setText,
    togglePack,
    toggleCategory,
    toggleFavoritesOnly,
    clearFilter,
    toggleSelected,
    clearSelection,
    setPreview,
  } = useStore();

  const [exportOpen, setExportOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const registry = usePluginRegistry();

  useEffect(() => {
    void initCatalog({ setCatalog, setPacks, setLoading, setError });
  }, [setCatalog, setPacks, setLoading, setError]);

  const assets = catalog?.assets ?? [];
  const visible = useMemo(() => applyFilter(assets, filter), [assets, filter]);
  const previewAsset = previewId ? assets.find((a) => a.id === previewId) : undefined;

  const {
    progress,
    start: startThumbs,
    pause: pauseThumbs,
    regenerate: regenerateThumbs,
  } = useThumbGeneration(assets);
  const pendingCount = useMemo(
    () => assets.filter((a) => a.thumb.state !== "ready").length,
    [assets],
  );

  const { toggleFavorite } = useFavorites();

  const rescan = () =>
    void initCatalog({ setCatalog, setPacks, setLoading, setError }, true);

  if (loading) {
    return (
      <Stack grow align="center" justify="center" gap={12}>
        <Spinner />
        <span>Loading library…</span>
      </Stack>
    );
  }

  if (error) {
    return (
      <Stack grow align="center" justify="center" gap={12}>
        <span style={{ color: "var(--danger)" }}>{error}</span>
        <Button onClick={rescan}>Retry scan</Button>
      </Stack>
    );
  }

  return (
    <>
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
            onExport={() => setExportOpen(true)}
            onImport={() => setImportOpen(true)}
            onRescan={rescan}
            onRegenerate={() => void regenerateThumbs()}
            thumbControl={
              <ThumbProgress
                progress={progress}
                pendingCount={pendingCount}
                onStart={() => void startThumbs()}
                onPause={pauseThumbs}
              />
            }
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

      <ExportDrawer
        open={exportOpen}
        selectedIds={[...selection]}
        exporters={registry.exporters}
        pluginErrors={registry.errors}
        onClose={() => setExportOpen(false)}
      />

      <ImportDrawer
        open={importOpen}
        importers={registry.importers}
        pluginErrors={registry.errors}
        onClose={() => setImportOpen(false)}
        onCommitted={() => {
          setImportOpen(false);
          rescan();
        }}
      />
    </>
  );
}

interface InitDeps {
  setCatalog: (c: Catalog) => void;
  setPacks: (p: PackMeta[]) => void;
  setLoading: (b: boolean) => void;
  setError: (e: string | null) => void;
}

async function initCatalog(deps: InitDeps, forceScan = false) {
  const { setCatalog, setPacks, setLoading, setError } = deps;
  setLoading(true);
  setError(null);
  try {
    // Pack identity comes from each pack's pack.json, independent of the asset
    // catalog. Load it alongside.
    setPacks(await loadPacks());
    // Force re-seed rebuilds the catalog from catalog.json (picking up new
    // packs and new fields like animation). Otherwise load the cache, scanning
    // only if none exists yet.
    const existing = forceScan ? null : await loadCatalog();
    const catalog = existing ?? (await scanLibrary(forceScan));
    setCatalog(catalog);
  } catch (e) {
    setError(String(e));
  } finally {
    setLoading(false);
  }
}
