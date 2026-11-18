/**
 * The main browsing view: facet rail + search/selection toolbar + virtualized
 * grid. The window holds no catalog: each library is made ready on mount (scanning to seed it the
 * first time), and what is shown comes from queries — a page of the list at a time, counts, single
 * items by id — asked again when the libraries change.
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import type { AssetKind } from "../domain/catalog";
import { NO_RULES, filterSignature, toBrowseFilter } from "../domain/browseQuery";
import { withAssetTypesInUse } from "../services/assetTypes";
import { assetTypeEntry } from "../state/assetTypes";
import { getAssets, queryIds, setCollection } from "../services/browse";
import { listLibraries, deleteAssets, transferAssets, PartialLibraryError, type DeleteResult } from "../services/tauriApi";
import { OfflineNotice } from "../components/OfflineNotice";
import { affectedLibraries, canOfferTransfer, readOnlyNotice, readOnlyOwners, type TransferMode } from "../domain/transfer";
import { loadAllLibraries, syncLibrary } from "../services/libraries";
import { libraryOf } from "../domain/libraryId";
import { useStore } from "../state/store";
import { useAsset, useAssets } from "../state/assetCache";
import { useAssetList } from "../state/assetList";
import { useCollections, useFacetCounts, useTypeCounts } from "../state/libraryCounts";
import { alsoInMap, useElsewhere } from "../state/duplicates";
import { useSavedExports } from "../state/savedExports";
import { AssetGrid } from "../components/AssetGrid";
import { AssetDetail } from "../components/AssetDetail";
import { AssetInspector } from "../components/AssetInspector";
import { LibrarySidebar } from "../components/LibrarySidebar";
import { CollectionDialog } from "../components/CollectionDialog";
import { DeleteAssetsDialog } from "../components/DeleteAssetsDialog";
import { TagSelectionDialog } from "../components/TagSelectionDialog";
import { TransferAssetsDialog } from "../components/TransferAssetsDialog";
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
import { useDebounced } from "../components/useDebounced";
import { usePluginRegistry } from "../components/usePluginRegistry";
import { Button, Spinner, Stack, Workbench, SelectionBar } from "@ldlework/toybox-sdk/ui";
import "./LibraryView.css";
import { useNarrowLibrary } from "../components/useNarrowLibrary";
import { CompactLibraryNavigation } from "../components/CompactLibraryNavigation";
import { pluginTypeIcons } from "../services/pluginIcons";
import { Drawer } from "@ldlework/toybox-sdk/ui";

/** Items listed in the delete dialog (a selection can be very large). */
const LISTED = 200;
/** The list follows the filter this long after it stops changing, so typing never waits for a query. */
const FILTER_SETTLE_MS = 120;
/** Selections up to this size are checked against the libraries when their contents change. */
const CHECKED = 5000;

export function LibraryView() {
  const {
    loading,
    error,
    filter,
    selection,
    previewId,
    libraries,
    libraryErrors,
    dataVersion,
    libraryCounts,
    setLibraries,
    setLoading,
    setError,
    setText,
    toggleLibrary,
    toggleSource,
    togglePublisher,
    toggleLicense,
    toggleFormat,
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
  const [collection, setCollectionName] = useState<string | null>(null);
  const [collectIds, setCollectIds] = useState<string[]>();
  const [tagIds, setTagIds] = useState<string[]>();
  const [deleteIds, setDeleteIds] = useState<string[]>();
  const [transfer, setTransfer] = useState<{ mode: TransferMode; ids: string[] }>();
  const [exportIds, setExportIds] = useState<string[]>([]);
  const [propertiesOpen,setPropertiesOpen]=useState(false);
  const [exportSetup, setExportSetup] = useState<string>();
  const [importOpen, setImportOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // null = a library is registered; "first-run" = show the picker.
  const [pickMode, setPickMode] = useState<"first-run" | null>(null);
  const registry = usePluginRegistry();
  const rules = registry.rules ?? NO_RULES;
  const narrow=useNarrowLibrary();
  const [maximized,setMaximized]=useState(false);
  const [navigationOpen,setNavigationOpen]=useState(false);
  const compact=narrow||maximized;
  useEffect(()=>{if(!compact)setNavigationOpen(false);},[compact]);
  const icons=useMemo(()=>pluginTypeIcons(registry.importers,registry.types),[registry.importers,registry.types]);
  const changeKind=(kind:typeof filter.kind)=>{setKind(kind);clearFilter();setCollectionName(null);setExportOpen(false);setNavigationOpen(false);};
  const changeFavorites=()=>{setCollectionName(null);toggleFavoritesOnly();setExportOpen(false);setNavigationOpen(false);};

  const deps = { setLibraries, setLoading, setError, setPickMode };

  useEffect(() => {
    void initCatalog(deps);
    void useSavedExports.getState().load().catch((e) => setError(String(e)));
    // deps is rebuilt each render but its setters are stable; init once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Items whose plugin is switched off are hidden everywhere in the library view — counts, filters,
  // collections, thumbnails (the queries take the rules). They stay in the libraries and on disk, and
  // return when the plugin is turned back on.
  const typeCounts = useTypeCounts(rules);
  const allItems = Object.values(libraryCounts).reduce((sum, count) => sum + count, 0);
  const shownItems = typeCounts ? [...typeCounts.values()].reduce((sum, count) => sum + count, 0) : allItems;
  const hiddenAny = !shownItems && allItems > 0;
  // Sections come from installed plugins; a type that has assets but no plugin
  // still gets a section so its items stay reachable.
  const types=useMemo(()=>withAssetTypesInUse(registry.types,typeCounts?[...typeCounts].filter(([,count])=>count>0).map(([id])=>id):[],registry.hiddenTypes),[registry.types,typeCounts,registry.hiddenTypes]);
  // Turning a plugin off can remove the section being shown; fall back to one
  // that still exists.
  useEffect(()=>{ if(typeCounts&&!types.some(type=>type.id===filter.kind)) setKind(types[0]?.id??"model"); },[types,typeCounts,filter.kind,setKind]);

  // The list: the filter settles a moment after it stops changing, then a page of the list is asked for as rows
  // scroll into view. The same item in several libraries is shown once (the libraries rank, see `domain/duplicates`);
  // with libraries chosen in the Libraries filter the list shows their own copies, de-duplicated among themselves.
  const queryFilter = useMemo(() => toBrowseFilter(filter, collection, rules), [filter, collection, rules]);
  const settledFilter = useDebounced(queryFilter, FILTER_SETTLE_MS);
  const listKey = useMemo(() => filterSignature(settledFilter), [settledFilter]);
  const list = useAssetList(pickMode || loading ? null : settledFilter);
  const visibleCount = list.total ?? 0;
  const facets = useFacetCounts(filter.kind, rules, filter.tags);
  const collections = useCollections(filter.kind, rules) ?? [];
  useEffect(() => { setInspectedId(undefined); }, [listKey]);
  const inspectedAsset = useAsset(inspectedId);
  const inspected = inspectedAsset ?? list.rowAt(0);
  const previewAsset = useAsset(previewId);
  const rangeIds = useCallback(async (from: number, to: number) => (await queryIds(settledFilter, from, to - from + 1)).ids, [settledFilter]);

  // A selected or previewed item that is no longer in a library is let go when the libraries change.
  useEffect(() => {
    const state = useStore.getState();
    const ids = [...state.selection];
    if (!ids.length || ids.length > CHECKED) return;
    let alive = true;
    void getAssets(ids).then((found) => {
      if (!alive) return;
      const present = new Set(found.map((asset) => asset.id));
      const current = useStore.getState();
      if ([...current.selection].some((id) => !present.has(id) && ids.includes(id))) current.setSelection([...current.selection].filter((id) => present.has(id) || !ids.includes(id)));
    }, () => undefined);
    return () => { alive = false; };
  }, [dataVersion]);

  const {
    progress,
    error: thumbnailError,
    start: startThumbs,
    pause: pauseThumbs,
    stop: stopThumbs,
  } = useThumbGeneration();

  const { toggleFavorite } = useFavorites();
  // Transfers need somewhere else to go; changing a library needs it to be writable.
  const offerTransfer = canOfferTransfer(libraries);
  const openTransfer = (mode: TransferMode, ids: string[]) => setTransfer({ mode, ids: [...ids] });
  const selectionLocked = useMemo(() => readOnlyNotice("delete", readOnlyOwners(libraries, [...selection].map((id) => ({ library: libraryOf(id) })))), [libraries, selection]);

  // What the dialogs show is fetched by id.
  const deleteShown = useMemo(() => deleteIds?.slice(0, LISTED), [deleteIds]);
  const deleteItems = useAssets(deleteShown).assets;
  const deleteCopies = useElsewhere(deleteShown ?? NO_IDS);
  const transferItems = useAssets(transfer?.ids).assets;
  const transferCopies = useElsewhere(transfer?.ids ?? NO_IDS);
  const tagItems = useAssets(tagIds).assets;
  const tagCopies = useElsewhere(tagIds ?? NO_IDS);

  const rescan = () => void initCatalog(deps, true);
  const openExport = (ids: string[], setupId?: string, kind: AssetKind = filter.kind) => { setExportIds(ids); setExportSetup(setupId); setExportKind(kind); setReturnToExports(false); setExportOpen(true); };
  const manageExports = () => { setNavigationOpen(false); setExportsManagerOpen(true); };

  const renderSidebar = () => <LibrarySidebar
        typeCounts={typeCounts} filter={filter} collections={collections} collection={collection} onCollection={(name) => { setCollectionName(name); setExportOpen(false);setNavigationOpen(false); }}
        icons={icons} types={types} onMaximize={()=>{setMaximized(true);setNavigationOpen(false);}}
        onKind={changeKind}
        filters={<FacetFilter counts={facets} rules={rules} filter={filter} libraries={libraries} onToggleLibrary={toggleLibrary} onToggleSource={toggleSource} onTogglePublisher={togglePublisher} onToggleLicense={toggleLicense} onToggleFormat={toggleFormat} onToggleCategory={toggleCategory} onToggleTag={toggleTag} onToggleFavoritesOnly={()=>{setCollectionName(null);toggleFavoritesOnly();}} onClear={()=>{clearFilter();setCollectionName(null);}}/>}
        onExports={manageExports}
        onSettings={() => {setNavigationOpen(false);setSettingsOpen(true);}}
      />;

  if (pickMode) {
    return (
      <LibraryPicker
        onPicked={() => {
          setPickMode(null);
          void initCatalog(deps, true);
        }}
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

  /** What a delete did in the libraries it reached: drop the ids that are gone and have the list and counts ask again. */
  const afterDelete = (results: DeleteResult[]) => {
    const state = useStore.getState();
    for (const { summary } of results) state.setLibraryCount(summary.library, summary.count);
    const gone = new Set(results.flatMap((result) => result.deleted));
    state.setSelection([...state.selection].filter(id => !gone.has(id)));
    if (state.previewId && gone.has(state.previewId)) state.setPreview(null);
    state.bumpData();
  };

  return (
    <>
      <Workbench className={`library-workbench${compact?" is-compact":""}`} navigation={!compact&&renderSidebar()} header={<>
        {compact&&<CompactLibraryNavigation kind={filter.kind} types={types} icons={icons} favorites={filter.favoritesOnly} onKind={changeKind} onFavorites={changeFavorites} onExports={manageExports} onSettings={()=>setSettingsOpen(true)} narrow={narrow} onRestore={()=>{if(narrow)setNavigationOpen(true);else setMaximized(false);}}/>}
        <LibraryToolbar
            title={assetTypeEntry(filter.kind,types).label}
            searchText={filter.text}
            onSearchChange={setText}
            visibleCount={visibleCount}
            visibleApproximate={!list.exact}
            totalCount={typeCounts?.get(filter.kind) ?? 0}
            selectedCount={selection.size}
            onClearSelection={clearSelection}
            onExport={() => openExport([...selection])}
            onImport={() => setImportOpen(true)}
            filtersOpen={navigationOpen} onToggleFilters={compact?()=>setNavigationOpen(open=>!open):undefined}
            propertiesOpen={propertiesOpen} onToggleProperties={()=>setPropertiesOpen(open=>!open)}
          /></>} inspector={propertiesOpen&&inspected && <AssetInspector asset={inspected} active={!previewAsset}
            onOpen={() => setPreview(inspected.id)} onFavorite={() => toggleFavorite(inspected.id)}
            onExport={() => openExport([inspected.id])} onCollect={() => setCollectIds([inspected.id])}
            onTransfer={offerTransfer ? (mode) => openTransfer(mode, [inspected.id]) : undefined} />}>
          {list.error ? (
            <Stack grow align="center" justify="center" gap={12}>
              <span style={{ color: "var(--danger)" }}>{list.error}</span>
              <Button onClick={() => useStore.getState().bumpData()}>Try again</Button>
            </Stack>
          ) : list.total === 0 ? (
            <Stack grow align="center" justify="center" gap={12}>
              <strong>{shownItems ? "No matching things" : hiddenAny ? "Everything here is hidden" : "Your library is empty"}</strong>
              <span style={{ color: "var(--text-2)" }}>
                {hiddenAny ? "The plugins for this library's items are turned off. Turn them on in Settings → Plugins." : "Import models, material folders, animations or audio to get started."}
              </span>
              <Button variant="primary" onClick={() => setImportOpen(true)}>
                Import assets…
              </Button>
            </Stack>
          ) : (
            <AssetGrid
              kind={filter.kind}
              total={visibleCount}
              rowAt={list.rowAt}
              version={list.version}
              onRange={list.show}
              rangeIds={rangeIds}
              listKey={listKey}
              selection={selection}
              onSelectionChange={setSelection}
              onToggleFavorite={toggleFavorite}
              onOpen={setPreview}
              onInspect={setInspectedId}
              inspectedId={inspected?.id}
            />
          )}
          {selection.size > 0 && <SelectionBar label={`${selection.size} selected`}><Button variant="ghost" onClick={clearSelection}>Clear</Button><Button variant="ghost" onClick={() => setCollectIds([...selection])}>Add to collection</Button><Button variant="ghost" onClick={() => setTagIds([...selection])}>Tag…</Button>{offerTransfer && <><Button variant="ghost" onClick={() => openTransfer("copy", [...selection])}>Copy to library…</Button><Button variant="ghost" onClick={() => openTransfer("move", [...selection])}>Move to library…</Button></>}<Button variant="ghost" disabled={!!selectionLocked} title={selectionLocked} onClick={() => setDeleteIds([...selection])}>Delete</Button><Button variant="primary" onClick={() => openExport([...selection])}>Export</Button></SelectionBar>}
      </Workbench>
      {Object.keys(libraryErrors).length > 0 && <p className="library-notice" role="alert">{Object.entries(libraryErrors).map(([id, message]) => `${libraries.find(l => l.id === id)?.name ?? id}: ${message}`).join(" · ")}</p>}
      <OfflineNotice libraries={libraries} />
      <ThumbStatusBar progress={progress} error={thumbnailError} onStart={()=>void startThumbs()} onPause={pauseThumbs}/>
      <Drawer open={compact&&navigationOpen} side="left" label="Library navigation" onClose={()=>setNavigationOpen(false)}><div className="library-navigation-drawer">{renderSidebar()}</div></Drawer>
      {deleteIds && <DeleteAssetsDialog assets={deleteItems} count={deleteIds.length} libraries={libraries} alsoIn={alsoInMap(deleteCopies)} onClose={() => setDeleteIds(undefined)} onDelete={async () => {
        await stopThumbs();
        // Each library deletes its own items; a failure part-way still shows what was deleted.
        try { afterDelete(await deleteAssets(deleteIds)); }
        catch (failure) { if (failure instanceof PartialLibraryError) afterDelete(failure.done as DeleteResult[]); throw failure; }
        finally { void startThumbs(); }
      }} />}
      {transfer && <TransferAssetsDialog mode={transfer.mode} assets={transferItems} libraries={libraries} alsoIn={alsoInMap(transferCopies)} onClose={() => setTransfer(undefined)} onRun={async (choice, onProgress) => {
        await stopThumbs();
        const target = libraries.find(library => library.id === choice.toLibrary);
        try {
          return await transferAssets({ assets: transferItems, toLibrary: choice.toLibrary, mode: transfer.mode, onConflict: choice.onConflict, includeClips: choice.includeClips, onProgress }, target?.name ?? choice.toLibrary);
        } finally {
          // Only the libraries that changed are made ready again; the selection is checked against them when they have.
          const sources = [...new Set(transferItems.map(asset => asset.library))];
          for (const library of affectedLibraries(sources, choice.toLibrary, transfer.mode)) await syncLibrary(library, false);
          void startThumbs();
        }
      }} />}
      {tagIds && <TagSelectionDialog assets={tagItems} alsoIn={alsoInMap(tagCopies)} onClose={() => setTagIds(undefined)} />}
      {collectIds && <CollectionDialog names={collections.map(({ name }) => name)} count={collectIds.length} onClose={() => setCollectIds(undefined)} onSave={async (name) => {
        await setCollection(collectIds, name, true);
        useStore.getState().bumpData();
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
        onCommitted={(library) => {
          setImportOpen(false);
          // The import updated the library's rows as it went; nothing needs rebuilding.
          void syncLibrary(library, false);
        }}
      />

      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        plugins={registry.plugins}
        pluginErrors={registry.errors}
        pluginProvided={registry.provided}
        onPluginsChanged={registry.reload}
      />
    </>
  );
}

const NO_IDS: string[] = [];

interface InitDeps {
  setLibraries: (libraries: Awaited<ReturnType<typeof listLibraries>>) => void;
  setLoading: (b: boolean) => void;
  setError: (e: string | null) => void;
  setPickMode: (m: "first-run" | null) => void;
}

async function initCatalog(deps: InitDeps, forceScan = false) {
  const { setLibraries, setLoading, setError, setPickMode } = deps;
  // No library registered → route to the picker, not the scan (which would
  // otherwise fail reading a nonexistent seed path).
  const libraries = await listLibraries();
  setLibraries(libraries);
  if (libraries.length === 0) {
    setPickMode("first-run");
    return;
  }
  setLoading(true);
  setError(null);
  try {
    // Every library is made ready (scanning only if it has no catalog yet;
    // forced, every library rescans). One that cannot be read is reported next
    // to the grid and the others still show.
    await loadAllLibraries(forceScan);
  } catch (e) {
    setError(String(e));
  } finally {
    setLoading(false);
  }
}
