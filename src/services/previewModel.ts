import type { ModelAsset } from "../domain/catalog";
export const MANNEQUIN_ID = "toybox:cc0-mannequin";
export const SKELETON_ID = "toybox:skeleton";
export const mannequin: ModelAsset = { id: MANNEQUIN_ID, kind: "model", name: "Toybox mannequin", fileName: "mannequin.glb", relPath: "", pack: "Built-in · Quaternius CC0", category: "preview", fileset: { gltf: "", bin: "", textures: [] }, animation: { clipCount: 0, clipNames: [] }, user: { favorite: false, tags: [] }, thumb: { state: "missing" } };
export const skeleton: ModelAsset = { ...mannequin, id: SKELETON_ID, name: "Skeleton only" };
