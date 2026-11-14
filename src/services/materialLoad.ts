/**
 * Build a three.js `MeshStandardMaterial` from a set of maps (a catalog material
 * asset, or any source that can resolve a file to a URL). Each unique texture
 * file is fetched once (several roles may share a packed file); a catalog asset
 * fetches through the Tauri asset protocol. Scalar maps that three would sample from the
 * wrong channel, or that are stored inverted, are repacked into grayscale; the
 * rest use the file's texture directly. PNG/JPG only.
 */

import {
  Color,
  type ColorSpace,
  MeshStandardMaterial,
  NoColorSpace,
  RepeatWrapping,
  SRGBColorSpace,
  type Texture,
  TextureLoader,
} from "three";

import type { MaterialAsset } from "../domain/catalog";
import type { MapRole, MaterialMap } from "../domain/material";
import { isScalarRole, needsRepack } from "../domain/materialSlots";
import { assetUrl } from "./assetUrl";
import { repackChannel } from "./channelRepack";

export interface MaterialLoadOptions {
  /** Longest side of repacked textures. Default 2048. Directly used files keep their size. */
  maxSize?: number;
}

const DEFAULT_MAX_SIZE = 2048;

/** Roles whose texture holds colour, so it is read as sRGB. */
const COLOUR_ROLES: ReadonlySet<MapRole> = new Set(["albedo", "emissive"]);

/** Attach a texture (and any scalar side effects) to its slot on the material. */
const SLOT: Record<MapRole, (m: MeshStandardMaterial, t: Texture) => void> = {
  albedo: (m, t) => (m.map = t),
  normal: (m, t) => (m.normalMap = t),
  roughness: (m, t) => (m.roughnessMap = t),
  metallic: (m, t) => {
    m.metalnessMap = t;
    m.metalness = 1;
  },
  ao: (m, t) => (m.aoMap = t),
  emissive: (m, t) => {
    m.emissiveMap = t;
    m.emissive = new Color(0xffffff);
  },
  opacity: (m, t) => {
    m.alphaMap = t;
    m.transparent = true;
  },
  // Height maps are ignored for now (no displacement/bump in the preview).
  height: () => {},
};

/** What a material is built from: its maps, and where their files can be fetched. */
export interface MaterialSource {
  maps: MaterialMap[];
  flipNormalGreen: boolean;
  /** The URL to fetch one of the maps' files from. */
  resolveUrl(file: string): Promise<string>;
}

/** Load a material asset into a ready-to-use standard material. Caller owns disposal. */
export function loadMaterial(
  asset: MaterialAsset,
  opts: MaterialLoadOptions = {},
): Promise<MeshStandardMaterial> {
  const { maps, flipNormalGreen } = asset.material;
  return buildMaterial({ maps, flipNormalGreen, resolveUrl: assetUrl }, opts);
}

/** Build a standard material from a source's maps. Caller owns disposal. */
export async function buildMaterial(
  source: MaterialSource,
  opts: MaterialLoadOptions = {},
): Promise<MeshStandardMaterial> {
  const maxSize = opts.maxSize ?? DEFAULT_MAX_SIZE;
  const { maps, flipNormalGreen, resolveUrl } = source;
  const used = maps.filter((m) => m.role !== "height");

  const files = await loadFiles(used, resolveUrl);
  const direct = directTextures(files);

  const material = new MeshStandardMaterial({ metalness: 0 });
  for (const map of used) {
    const texture = textureFor(map, files, direct, maxSize);
    SLOT[map.role](material, texture);
  }
  if (flipNormalGreen) material.normalScale.y = -1;
  return material;
}

/** Fetch each unique file once. */
async function loadFiles(
  maps: MaterialMap[],
  resolveUrl: MaterialSource["resolveUrl"],
): Promise<Map<string, Texture>> {
  const loader = new TextureLoader();
  const unique = [...new Set(maps.map((m) => m.file))];
  const entries = await Promise.all(
    unique.map(async (file) => [file, await loader.loadAsync(await resolveUrl(file))] as const),
  );
  return new Map(entries);
}

type DirectCache = (file: string, space: ColorSpace) => Texture;

/**
 * One texture per (file, colour space), so a packed file used by several slots
 * is a single shared texture. Variants share the decoded image.
 */
function directTextures(files: Map<string, Texture>): DirectCache {
  const cache = new Map<string, Texture>();
  return (file, space) => {
    const key = `${file}|${space}`;
    let texture = cache.get(key);
    if (!texture) {
      texture = files.get(file)!.clone();
      texture.colorSpace = space;
      texture.needsUpdate = true;
      repeat(texture);
      cache.set(key, texture);
    }
    return texture;
  };
}

function textureFor(
  map: MaterialMap,
  files: Map<string, Texture>,
  direct: DirectCache,
  maxSize: number,
): Texture {
  if (isScalarRole(map.role) && needsRepack(map)) {
    const source = files.get(map.file)!.image as HTMLImageElement;
    return repeat(repackChannel(source, map.channel, map.invert, maxSize));
  }
  const space = COLOUR_ROLES.has(map.role) ? SRGBColorSpace : NoColorSpace;
  return direct(map.file, space);
}

function repeat<T extends Texture>(texture: T): T {
  texture.wrapS = RepeatWrapping;
  texture.wrapT = RepeatWrapping;
  return texture;
}
