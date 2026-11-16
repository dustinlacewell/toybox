/**
 * Demo shim for services/tauriApi — the app's entire Rust command surface,
 * replaced with browser-native behaviour. The thumbnail pipeline is real: the
 * renderer (real app code) hands saveThumb its PNG bytes, which we keep in the
 * in-memory store so cards display genuine, live-rendered previews. Filesystem
 * operations (catalog persistence, export, origin-correction, reveal) have no
 * meaning without the library on disk and are inert here.
 */

import type { Catalog, PackMeta, ThumbState } from "@app/domain/catalog";
import { DEMO_PACKS, demoCatalog } from "../catalog";
import { desktopAction } from "../nativeNotice";
import { putThumb, getThumbUrl } from "./thumbStore";
import { assetUrl } from "./assetUrl";

export const deleteAssets = async (assetIds:string[]):Promise<Catalog> => {
  catalog = {...catalog,assets:catalog.assets.filter(asset=>!assetIds.includes(asset.id))}; return catalog;
};

export interface PreviewChoice { assetId: string; rigId: string; exact: boolean; }
export const compatiblePreviews = async (_assetId: string): Promise<PreviewChoice[]> => [];
export const readPreviewPair = async (animationPath:string, clipIndex:number, modelPath:string):Promise<unknown> => {
  const [model, source] = await Promise.all([readPreviewSource(modelPath,null),readPreviewSource(animationPath,clipIndex)]) as any[];
  const bufferOffset=(model.buffers??=[]).length, viewOffset=(model.bufferViews??=[]).length, accessorOffset=(model.accessors??=[]).length;
  model.buffers.push(...source.buffers);
  model.bufferViews.push(...source.bufferViews.map((view:any)=>({...view,buffer:view.buffer+bufferOffset})));
  model.accessors.push(...source.accessors.map((accessor:any)=>({...accessor,bufferView:accessor.bufferView+viewOffset})));
  const animation=source.animations[0];
  animation.samplers=animation.samplers.map((sampler:any)=>({...sampler,input:sampler.input+accessorOffset,output:sampler.output+accessorOffset}));
  animation.channels=animation.channels.map((channel:any)=>({...channel,target:{...channel.target,node:model.nodes.findIndex((node:any)=>node.name===source.nodes[channel.target.node].name)}}));
  model.animations=[animation];return model;
};
export const readAnimationPreview = async (animationId:string, modelId:string) => {
 const animation=catalog.assets.find(a=>a.id===animationId),model=catalog.assets.find(a=>a.id===modelId);
 if(animation?.kind!=="animation"||model?.kind!=="model")throw Error("Choose an animation and model");
 return readPreviewPair(animation.fileset.gltf,animation.clip.index,model.fileset.gltf);
};

export const readAssetGltf = async (assetId: string): Promise<unknown> => {
  const asset = catalog.assets.find(asset => asset.id === assetId);
  if (!asset || (asset.kind === "material" || asset.kind === "audio")) throw new Error("Scene asset not found");
  const response = await fetch(await assetUrl(asset.fileset.gltf));
  if (!response.ok) throw new Error("Could not read scene");
  return response.json();
};
export const readPreviewSource=async(relativePath:string,clipIndex:number|null):Promise<unknown>=>{
  const response=await fetch(await assetUrl(relativePath));if(!response.ok)throw Error(`Could not read ${relativePath}`);
  const doc=await response.json();if(clipIndex===null)delete doc.animations;else doc.animations=[doc.animations[clipIndex]];return doc;
};

// --- Thumbnail cache --------------------------------------------------------

export const saveThumb = async (assetId: string, png: Uint8Array): Promise<void> => {
  putThumb(assetId, png);
};

export const setThumbState = async (
  _assetId: string,
  _state: ThumbState,
  _error?: string,
): Promise<void> => {};

export const listPendingThumbs = async (): Promise<string[]> => catalog.assets.filter(a=>a.kind!=="audio"&&!getThumbUrl(a.id)).map(a=>a.id);

export const clearThumbs = async (): Promise<void> => {};
export const invalidateThumb = async (_assetId:string): Promise<void> => {};

export const thumbDir = async (): Promise<string> => "/demo-assets";

export const thumbPath = async (assetId: string): Promise<string> => assetId;

// --- Catalog / paths: served from the fixture, never persisted ---------------

export const scanLibrary = async (_forceReseed: boolean): Promise<Catalog> =>
  demoCatalog();

let catalog=demoCatalog();
export const loadCatalog = async (): Promise<Catalog | null> => catalog;

export const saveCatalog = async (value: Catalog): Promise<void> => { catalog=value; };

export const libraryRoot = async (): Promise<string> => "polygon_prototype";

// The demo's library is the bundled fixture — always "configured", so the
// first-run picker never appears and re-pointing is a no-op.
export const getLibraryRoot = async (): Promise<string | null> => "polygon_prototype";

export const setLibraryRoot = async (_path: string): Promise<void> => {};

export const createLibrary = async (_path: string): Promise<void> => {};

// Plugin source-read primitives — inert in the browser (no filesystem to walk).
export const pluginReadDir = async (
  _sourceRoot: string,
  _path: string,
): Promise<{ name: string; relPath: string; isDir: boolean }[]> => [];

export const pluginReadBytes = async (
  _sourceRoot: string,
  _path: string,
): Promise<Uint8Array> => new Uint8Array();

// Settings / FBX conversion — no filesystem or process in the browser.
export const getFbx2gltfPath = async (): Promise<string | null> => null;
export const setFbx2gltfPath = async (_path: string | null): Promise<void> => {};
export const convertToGltf = async (
  _srcPath: string,
  _pack: string,
  _category: string,
  _stem: string,
): Promise<string[]> => [];

export const loadPacks = async (): Promise<PackMeta[]> => DEMO_PACKS;

export const resolveAssetPath = async (relPath: string): Promise<string> => relPath;

// --- Export / origin tools: inert in the browser -----------------------------

export interface ExportReport {
  written: string[];
  skipped: string[];
  warnings: string[];
}

const unavailable=async ():Promise<any>=>{desktopAction();throw Error("Use the desktop app to write files.");};


export const exportCopy = async (): Promise<ExportReport> => unavailable();
export const exportGlb = async (): Promise<ExportReport> => unavailable();

export type PlacerFormat = "glb" | "copy";

export interface ExportPlacerArgs {
  assetIds: string[];
  targetDir: string;
  subDir: string;
  preserveStructure: boolean;
  format: PlacerFormat;
  libraryJsonPath: string;
}

export const exportPlacer = async (_args: ExportPlacerArgs): Promise<ExportReport> =>
  unavailable();

export type Axis = "x" | "y" | "z";
export type Align = "min" | "center" | "max";

export interface Aabb {
  min: [number, number, number];
  max: [number, number, number];
}

export const recenterAsset = async (
  _assetId: string,
  _axis: Axis,
  _align: Align,
): Promise<Aabb> => unavailable();

// Discover the same bundled importers and icons as the desktop application.
const manifests=import.meta.glob("../../../../plugins/*/manifest.json",{eager:true,import:"default"}) as Record<string,import("@app/services/tauriApi").PluginManifestDto>;
export const listPlugins=async()=>Object.values(manifests).filter(manifest=>manifest.kind==="importer").map(manifest=>({...manifest,entry:"",entryAbsPath:"",fields:[]}));
let savedExports:import("@app/domain/savedExport").SavedExport[]=[];
export const getSavedExports=async()=>savedExports;
export const setSavedExports=async(value:typeof savedExports)=>{savedExports=value;};
export const previewExport=async(ids:string[])=>catalog.assets.filter(a=>ids.includes(a.id)).map(a=>a.fileName);
export const assetFingerprints=async(ids:string[])=>Object.fromEntries(ids.map(id=>[id,id]));
export const mergeSeedEntries=unavailable;
export const pluginReadText=async(path:string)=>(await fetch(await assetUrl(path))).text();
export const pluginExists=async(path:string)=>(await fetch(await assetUrl(path),{method:"HEAD"})).ok;
export const pluginImageThumb=unavailable;
export const pluginWriteBytes=unavailable;
export const pluginWriteText=unavailable;
export const assembleGlbForAsset=unavailable;
export const performAssetCopy=unavailable;
export const transcodeImage=unavailable;
export const placerMergeFile=unavailable;
