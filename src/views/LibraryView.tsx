/**
 * The main browsing view: facet rail + search/selection toolbar + virtualized
 * grid. Loads the catalog on mount (scanning to seed it the first time), and
 * derives the visible list with the pure facet selectors.
 */

import { useEffect, useMemo, useState } from "react";

import type { AssetKind, Catalog, PackMeta } from "../domain/catalog";
import { ASSET_KIND_LABELS } from "../domain/catalog";
import { applyFilter, ofKind } from "../domain/facets";
import {
  getLibraryRoot,
  loadCatalog,
  loadPacks,
  scanLibrary,
  saveCatalog,
  deleteAssets,
} from "../services/tauriApi";
import { useStore } from "../state/store";
import { useSavedExports } from "../state/savedExports";
import { AssetGrid } from "../components/AssetGrid";
import { AssetDetail } from "../components/AssetDetail";
import { AssetInspector } from "../components/AssetInspector";
import { LibrarySidebar } from "../components/LibrarySidebar";
import { CollectionDialog } from "../components/CollectionDialog";
import { DeleteAssetsDialog } from "../components/DeleteAssetsDialog";
import { ExportDrawer } from "../components/ExportDrawer";
import { ExportsModal } from "../components/ExportsModal";
import { ImportDrawer } from "../components/ImportDrawer";
import { FacetFilter } from "../components/FacetFilter";
import { LibraryPicker } from "../components/LibraryPicker";
import { LibraryToolbar } from "../components/LibraryToolbar";
import { SettingsModal } from "../components/SettingsModal";
import { ThumbStatusBar } from "../components/ThumbStatusBar";
import { useThumbGeneration } from "../components/useThumbGeneration";
import { useFavorites } from "../components/useFavorites";
import { usePluginRegistry } from "../components/usePluginRegistry";
import { Button, Spinner, Stack, Workbench, SelectionBar } from "@ldlework/toybox-sdk/ui";
import "./LibraryView.css";
import { useNarrowLibrary } from "../components/useNarrowLibrary";
import { CompactLibraryNavigation } from "../components/CompactLibraryNavigation";
import { pluginTypeIcons } from "../services/pluginIcons";
import { Drawer } from "@ldlework/toybox-sdk/ui";

export function LibraryView() {
  const {
    catalog,
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
    toggleSource,
    togglePublisher,
    toggleLicense,
    toggleCategory,
    toggleTag,
    toggleFavoritesOnly,
    clearFilter,
    setSelection,
    clearSelection,
    setPreview,
    setKind,
  } = useStore();

  const [exportOpen, setExportOpen] = useState(false);
  const [exportsManagerOpen, setExportsManagerOpen] = useState(false);
  const [exportKind, setExportKind] = useState<AssetKind>("model");
  const [returnToExports, setReturnToExports] = useState(false);
  const [inspectedId, setInspectedId] = useState<string>();
  const [collection, setCollection] = useState<string | null>(null);
  const [collectIds, setCollectIds] = useState<string[]>();
  const [deleteIds, setDeleteIds] = useState<string[]>();
  const [exportIds, setExportIds] = useState<string[]>([]);
  const [propertiesOpen,setPropertiesOpen]=useState(false);
  const [exportSetup, setExportSetup] = useState<string>();
  const [importOpen, setImportOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [libraryRoot, setLibraryRoot] = useState<string | null>(null);
  // null = library configured; "first-run" / "repoint" = show the picker.
  const [pickMode, setPickMode] = useState<"first-run" | "repoint" | null>(null);
  const registry = usePluginRegistry();
  const narrow=useNarrowLibrary();
  const [maximized,setMaximized]=useState(false);
  const [navigationOpen,setNavigationOpen]=useState(false);
  const compact=narrow||maximized;
  useEffect(()=>{if(!compact)setNavigationOpen(false);},[compact]);
  const icons=useMemo(()=>pluginTypeIcons(registry.importers),[registry.importers]);
  const changeKind=(kind:typeof filter.kind)=>{setKind(kind);clearFilter();setCollection(null);setExportOpen(false);setNavigationOpen(false);};
  const changeFavorites=()=>{setCollection(null);toggleFavoritesOnly();setExportOpen(false);setNavigationOpen(false);};

  const deps = { setCatalog, setPacks, setLoading, setError, setPickMode };

  useEffect(() => {
    void initCatalog(deps);
    void getLibraryRoot().then(setLibraryRoot);
    void useSavedExports.getState().load().catch((e) => setError(String(e)));
    // deps is rebuilt each render but its setters are stable; init once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const assets = catalog?.assets ?? [];
  const visible = useMemo(() => applyFilter(assets, filter).filter((a) => !collection || a.user.collections?.includes(collection)), [assets, filter, collection]);
  const collections = useMemo(() => [...new Set(assets.flatMap((a) => a.user.collections ?? []))].sort(), [assets]);
  const kindCount = useMemo(
    () => ofKind(assets, filter.kind).length,
    [assets, filter.kind],
  );
  const previewAsset = previewId ? assets.find((a) => a.id === previewId) : undefined;
  const inspected = visible.find((a) => a.id === inspectedId) ?? visible[0];

  const {
    progress,
    error: thumbnailError,
    start: startThumbs,
    pause: pauseThumbs,
    stop: stopThumbs,
  } = useThumbGeneration(assets);

  const { toggleFavorite } = useFavorites();

  const rescan = () => void initCatalog(deps, true);
  const openExport = (ids: string[], setupId?: string, kind: AssetKind = filter.kind) => { setExportIds(ids); setExportSetup(setupId); setExportKind(kind); setReturnToExports(false); setExportOpen(true); };
  const manageExports = () => { setNavigationOpen(false); setExportsManagerOpen(true); };

  const renderSidebar = () => <LibrarySidebar
        assets={assets} filter={filter} collections={collections} collection={collection} onCollection={(name) => { setCollection(name); setExportOpen(false);setNavigationOpen(false); }}
        icons={icons} onMaximize={()=>{setMaximized(true);setNavigationOpen(false);}}
        onKind={changeKind}
        filters={<FacetFilter assets={assets} filter={filter} onToggleSource={toggleSource} onTogglePublisher={togglePublisher} onToggleLicense={toggleLicense} onToggleCategory={toggleCategory} onToggleTag={toggleTag} onToggleFavoritesOnly={()=>{setCollection(null);toggleFavoritesOnly();}} onClear={()=>{clearFilter();setCollection(null);}}/>}
        onExports={manageExports}
        onSettings={() => {setNavigationOpen(false);setSettingsOpen(true);}} onRescan={rescan}
      />;

  if (pickMode) {
    return (
      <LibraryPicker
        onPicked={() => {
          setPickMode(null);
          void getLibraryRoot().then(setLibraryRoot);
          void initCatalog(deps, true);
        }}
        onCancel={pickMode === "repoint" ? () => setPickMode(null) : undefined}
      />
    );
  }

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
      <Workbench className={`library-workbench${compact?" is-compact":""}`} navigation={!compact&&renderSidebar()} header={<>
        {compact&&<CompactLibraryNavigation kind={filter.kind} icons={icons} favorites={filter.favoritesOnly} onKind={changeKind} onFavorites={changeFavorites} onExports={manageExports} onSettings={()=>setSettingsOpen(true)} narrow={narrow} onRestore={()=>{if(narrow)setNavigationOpen(true);else setMaximized(false);}}/>}
        <LibraryToolbar
            title={ASSET_KIND_LABELS[filter.kind]}
            searchText={filter.text}
            onSearchChange={setText}
            visibleCount={visible.length}
            totalCount={kindCount}
            selectedCount={selection.size}
            onClearSelection={clearSelection}
            onExport={() => openExport([...selection])}
            onImport={() => setImportOpen(true)}
            filtersOpen={navigationOpen} onToggleFilters={compact?()=>setNavigationOpen(open=>!open):undefined}
            propertiesOpen={propertiesOpen} onToggleProperties={()=>setPropertiesOpen(open=>!open)}
          /></>} inspector={propertiesOpen&&inspected && <AssetInspector asset={inspected} active={!previewAsset}
            onOpen={() => setPreview(inspected.id)} onFavorite={() => toggleFavorite(inspected.id)}
            onExport={() => openExport([inspected.id])} onCollect={() => setCollectIds([inspected.id])} />}>
          {visible.length === 0 ? (
            <Stack grow align="center" justify="center" gap={12}>
              <strong>{assets.length ? "No matching things" : "Your library is empty"}</strong>
              <span style={{ color: "var(--text-2)" }}>
                Import models, material folders, animations or audio to get started.
              </span>
              <Button variant="primary" onClick={() => setImportOpen(true)}>
                Import assets…
              </Button>
            </Stack>
          ) : (
            <AssetGrid
              assets={visible}
              selection={selection}
              onSelectionChange={setSelection}
              onToggleFavorite={toggleFavorite}
              onOpen={setPreview}
              onInspect={setInspectedId}
              inspectedId={inspected?.id}
            />
          )}
          {selection.size > 0 && <SelectionBar label={`${selection.size} selected`}><Button variant="ghost" onClick={clearSelection}>Clear</Button><Button variant="ghost" onClick={() => setCollectIds([...selection])}>Add to collection</Button><Button variant="ghost" onClick={() => setDeleteIds([...selection])}>Delete</Button><Button variant="primary" onClick={() => openExport([...selection])}>Export</Button></SelectionBar>}
      </Workbench>
      <ThumbStatusBar progress={progress} error={thumbnailError} onStart={()=>void startThumbs()} onPause={pauseThumbs}/>
      <Drawer open={compact&&navigationOpen} side="left" label="Library navigation" onClose={()=>setNavigationOpen(false)}><div className="library-navigation-drawer">{renderSidebar()}</div></Drawer>
      {deleteIds && <DeleteAssetsDialog assets={assets.filter(asset => deleteIds.includes(asset.id))} onClose={() => setDeleteIds(undefined)} onDelete={async () => {
        await stopThumbs();
        const updated = await deleteAssets(deleteIds);
        const current = new Map(useStore.getState().catalog?.assets.map(asset => [asset.id, asset.thumb]));
        setCatalog({ ...updated, assets: updated.assets.map(asset => ({ ...asset, thumb: current.get(asset.id) ?? asset.thumb })) });
        clearSelection();
        if (previewId && deleteIds.includes(previewId)) setPreview(null);
        void startThumbs();
      }} />}
      {collectIds && <CollectionDialog names={collections} count={collectIds.length} onClose={() => setCollectIds(undefined)} onSave={async (name) => {
        const current = useStore.getState().catalog;
        if (!current) return;
        const updated = { ...current, assets: current.assets.map((a) => collectIds.includes(a.id) && !a.user.collections?.includes(name) ? { ...a, user: { ...a.user, collections: [...(a.user.collections ?? []), name] } } : a) };
        await saveCatalog(updated); setCatalog(updated);
      }} />}

      {previewAsset && (
        <AssetDetail asset={previewAsset} onClose={() => setPreview(null)} onExport={() => { setPreview(null); openExport([previewAsset.id]); }} />
      )}

      <ExportDrawer
        open={exportOpen}
        kind={exportKind}
        setupId={exportSetup}
        selectedIds={exportIds}
        exporters={registry.exporters}
        pluginErrors={registry.errors}
        onClose={() => { setExportOpen(false); if (returnToExports) setExportsManagerOpen(true); }}
      />

      <ExportsModal open={exportsManagerOpen} kind={filter.kind} selectedCount={selection.size} exporters={registry.exporters} pluginErrors={registry.errors}
        onClose={()=>setExportsManagerOpen(false)} onExport={setup=>{
          setExportsManagerOpen(false); openExport([...selection],setup.id,setup.kind); setReturnToExports(true);
        }}/>

      <ImportDrawer
        open={importOpen}
        importers={registry.importers}
        kind={filter.kind}
        pluginErrors={registry.errors}
        onClose={() => setImportOpen(false)}
        onCommitted={() => {
          setImportOpen(false);
          rescan();
        }}
      />

      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        libraryRoot={libraryRoot}
        onChangeLibrary={() => {
          setSettingsOpen(false);
          setPickMode("repoint");
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
  setPickMode: (m: "first-run" | "repoint" | null) => void;
}

async function initCatalog(deps: InitDeps, forceScan = false) {
  const { setCatalog, setPacks, setLoading, setError, setPickMode } = deps;
  // No library configured → route to the picker, not the scan (which would
  // otherwise fail reading a nonexistent seed path).
  if ((await getLibraryRoot()) === null) {
    setPickMode("first-run");
    return;
  }
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
