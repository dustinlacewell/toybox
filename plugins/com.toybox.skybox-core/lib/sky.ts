/**
 * Turning a skybox asset's files into a three.js texture, whatever they are:
 * a panorama, six cube faces, or a cube unfolded into a cross or strip, stored
 * as Radiance HDR, OpenEXR, PNG, JPEG or WebP.
 */

import {
  CanvasTexture, CubeTexture, DataTexture, EquirectangularReflectionMapping, FloatType, HalfFloatType, LinearFilter,
  LinearSRGBColorSpace, RGBAFormat, RGFormat, RedFormat, SRGBColorSpace, Texture, type PixelFormat, type TextureDataType,
} from "three";
import { EXRLoader } from "three/addons/loaders/EXRLoader.js";
import { HDRLoader } from "three/addons/loaders/HDRLoader.js";

import { FACES, faceCells, isHdr, type Encoding, type Face, type SkyMeta } from "./detect.js";
import { cutCell, flipRows, shrink, type PixelGrid, type Samples } from "./pixels.js";

/** Where a sky's files come from: the library (viewer, thumbnails) or a folder
 *  being imported (the import preview). */
export interface SkySource {
  meta: SkyMeta;
  /** The image file of a single-image sky. Cube maps name theirs in `meta.faces`. */
  file: string;
  read(file: string): Promise<Uint8Array>;
  /** A URL an `<img>` can load `file` from. */
  url(file: string): Promise<string>;
}

export interface LoadedSky {
  texture: Texture;
  /** Whether the pixels hold real light levels (and so are tone mapped). */
  hdr: boolean;
  dispose(): void;
}

export interface LoadOptions {
  /** Upper bound for the panorama width (a cube face gets a quarter of it).
   *  Larger images are shrunk to fit; previews and thumbnails pass a small one. */
  maxWidth?: number;
  signal?: AbortSignal;
}

interface FloatImage extends PixelGrid { type: TextureDataType; format: PixelFormat; }

const FORMATS: Record<number, PixelFormat> = { 1: RedFormat, 2: RGFormat, 4: RGBAFormat };

/** Decode an HDR or EXR file into top-row-first pixels. */
function decodeFloat(bytes: Uint8Array, encoding: Encoding): FloatImage {
  // The loaders take an ArrayBuffer that starts at the image's first byte.
  const buffer = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes.buffer : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const parsed = encoding === "exr" ? new EXRLoader().setDataType(HalfFloatType).parse(buffer as ArrayBuffer) : new HDRLoader().setDataType(HalfFloatType).parse(buffer as ArrayBuffer);
  const data = parsed.data as Samples;
  const { width, height } = parsed;
  const channels = Math.round(data.length / (width * height));
  if (!FORMATS[channels]) throw new Error(`Unsupported image: ${channels} channels per pixel`);
  const grid: PixelGrid = { data, width, height, channels };
  // Radiance scanlines run top to bottom; EXRLoader hands back bottom to top.
  const top = encoding === "exr" ? flipRows(grid) : grid;
  return { ...top, type: data instanceof Float32Array ? FloatType : HalfFloatType, format: FORMATS[channels] };
}

function loadImage(url: string, signal?: AbortSignal): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => (signal?.aborted ? reject(new DOMException("Aborted", "AbortError")) : resolve(image));
    image.onerror = () => reject(new Error("The image could not be decoded"));
    image.src = url;
  });
}

/** Draw a region of an image onto a new canvas, optionally scaled and turned. */
function toCanvas(image: CanvasImageSource, sx: number, sy: number, sw: number, sh: number, size: { width: number; height: number }, turn = false): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = size.width; canvas.height = size.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Could not prepare the image");
  if (turn) { context.translate(size.width, size.height); context.rotate(Math.PI); }
  context.drawImage(image, sx, sy, sw, sh, 0, 0, size.width, size.height);
  return canvas;
}

const fit = (width: number, height: number, maxWidth?: number) => {
  const scale = maxWidth && width > maxWidth ? maxWidth / width : 1;
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) };
};

function check(signal?: AbortSignal) { if (signal?.aborted) throw new DOMException("Aborted", "AbortError"); }

function floatTexture(image: FloatImage): DataTexture {
  const texture = new DataTexture(image.data, image.width, image.height, image.format, image.type);
  texture.colorSpace = LinearSRGBColorSpace;
  texture.minFilter = LinearFilter; texture.magFilter = LinearFilter;
  texture.generateMipmaps = false;
  return texture;
}

/** Six top-row-first float faces, in `FACES` order, as one cube texture. */
function floatCube(faces: FloatImage[]): LoadedSky {
  const [first] = faces;
  if (faces.some((face) => face.width !== first.width || face.height !== first.height || face.channels !== first.channels || face.width !== face.height)) {
    throw new Error("The six cube faces must be square and the same size");
  }
  const textures = faces.map((face) => { const texture = floatTexture(face); texture.needsUpdate = true; return texture; });
  // three reads `.image` of each face when a cube is built from data textures.
  const cube = new CubeTexture(textures as unknown as HTMLImageElement[]);
  cube.format = first.format; cube.type = first.type;
  cube.colorSpace = LinearSRGBColorSpace;
  cube.minFilter = LinearFilter; cube.magFilter = LinearFilter;
  cube.generateMipmaps = false;
  cube.needsUpdate = true;
  return { texture: cube, hdr: true, dispose: () => { cube.dispose(); for (const texture of textures) texture.dispose(); } };
}

function imageCube(faces: (HTMLCanvasElement | HTMLImageElement)[]): LoadedSky {
  const sizes = faces.map((face) => [face.width, face.height]);
  if (sizes.some(([width, height]) => width !== sizes[0][0] || height !== sizes[0][1] || width !== height)) {
    throw new Error("The six cube faces must be square and the same size");
  }
  const cube = new CubeTexture(faces);
  cube.colorSpace = SRGBColorSpace;
  cube.needsUpdate = true;
  return { texture: cube, hdr: false, dispose: () => cube.dispose() };
}

/** Load a sky as a texture ready to be a scene background and environment. */
export async function loadSky(source: SkySource, options: LoadOptions = {}): Promise<LoadedSky> {
  const { meta } = source;
  const { maxWidth, signal } = options;
  const float = isHdr(meta.encoding);
  const faceWidth = maxWidth ? Math.max(64, Math.round(maxWidth / 4)) : undefined;

  if (meta.layout === "cubemap") {
    if (!meta.faces) throw new Error("This cube map does not say which image is which face");
    const names = FACES.map((face: Face) => meta.faces![face]);
    if (float) {
      const faces: FloatImage[] = [];
      for (const name of names) { // One at a time: each face can be large.
        const image = decodeFloat(await source.read(name), meta.encoding);
        check(signal);
        faces.push(faceWidth ? { ...image, ...shrink(image, faceWidth) } : image);
      }
      return floatCube(faces);
    }
    const images = await Promise.all(names.map(async (name) => loadImage(await source.url(name), signal)));
    check(signal);
    return imageCube(faceWidth && images[0].width > faceWidth
      ? images.map((image) => toCanvas(image, 0, 0, image.width, image.height, fit(image.width, image.height, faceWidth)))
      : images);
  }

  const cells = meta.layout === "equirect" ? undefined : faceCells(meta.layout);
  if (float) {
    const full = decodeFloat(await source.read(source.file), meta.encoding);
    check(signal);
    if (cells) {
      const faces = FACES.map((face) => {
        const cut = cutCell(full, cells.columns, cells.rows, cells.cells.find((cell) => cell.face === face)!);
        const sized = faceWidth ? shrink(cut, faceWidth) : cut;
        return { ...sized, type: full.type, format: full.format };
      });
      return floatCube(faces);
    }
    const sized = maxWidth ? { ...full, ...shrink(full, maxWidth) } : full;
    const texture = floatTexture(sized);
    texture.mapping = EquirectangularReflectionMapping;
    texture.flipY = true; // Rows are top first; the panorama's top must be up.
    texture.needsUpdate = true;
    return { texture, hdr: true, dispose: () => texture.dispose() };
  }

  const image = await loadImage(await source.url(source.file), signal);
  check(signal);
  if (cells) {
    const width = Math.floor(image.width / cells.columns), height = Math.floor(image.height / cells.rows);
    const size = fit(width, height, faceWidth);
    return imageCube(FACES.map((face) => {
      const cell = cells.cells.find((item) => item.face === face)!;
      return toCanvas(image, cell.column * width, cell.row * height, width, height, size, cell.turn);
    }));
  }
  const size = fit(image.width, image.height, maxWidth);
  const texture: Texture = size.width === image.width ? new Texture(image) : new CanvasTexture(toCanvas(image, 0, 0, image.width, image.height, size));
  texture.mapping = EquirectangularReflectionMapping;
  texture.colorSpace = SRGBColorSpace;
  texture.minFilter = LinearFilter; texture.magFilter = LinearFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return { texture, hdr: false, dispose: () => texture.dispose() };
}
