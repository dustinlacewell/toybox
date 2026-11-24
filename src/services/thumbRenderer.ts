/**
 * Offscreen thumbnail renderer. ONE persistent WebGLRenderer + scene + camera +
 * lights are reused across every asset (creating a renderer per asset would hit
 * the browser's ~16 live-context limit). Per asset we load the scene, frame it
 * deterministically, render, capture PNG bytes, then dispose that model's GPU
 * resources. The renderer itself lives until `dispose()`.
 *
 * The canvas is never attached to the DOM; `toBlob` works off-DOM.
 */

import {
  PerspectiveCamera,
  Scene,
  WebGLRenderer,
} from "three";

import type { ThumbnailTools } from "@ldlework/toybox-sdk";

import type { Asset } from "../domain/catalog";
import { parseQualified, qualify } from "../domain/libraryId";
import { disposeScene } from "./disposeScene";
import { fitCameraToObject } from "./fitCamera";
import { loadThumbSubject, materialSourceOf, materialSubject } from "./thumbSubject";
import { derivedAssets } from "./browse";
import { fetchAsset } from "./pluginHost";
import { cacheAssets } from "../state/assetCache";
import { audioThumbnail } from "./audioThumbnail";
import { pluginAssetFiles } from "./pluginAssetFiles";
import { assetTypeEntry, assetTypesLoaded } from "../state/assetTypes";
import { createScenePreviewStage } from "./scenePreviewStage";
type PreparedSubject=import("three").Object3D|{png:Uint8Array};

const SIZE = 256;

export class ThumbRenderer {
  private renderer!: WebGLRenderer;
  private scene!: Scene;
  private camera!: PerspectiveCamera;

  private canvas!: HTMLCanvasElement;

  private initialize(): void {
    if(this.renderer)return;
    this.canvas = document.createElement("canvas");
    this.canvas.width = SIZE;
    this.canvas.height = SIZE;
    // A fully-detached WebGL canvas can have its context reclaimed by the
    // compositor; keeping it in the DOM (but invisible) avoids spurious context
    // loss across a long generation run.
    Object.assign(this.canvas.style, {
      position: "fixed",
      left: "-9999px",
      top: "0",
      width: `${SIZE}px`,
      height: `${SIZE}px`,
      pointerEvents: "none",
    });
    document.body.appendChild(this.canvas);
    this.canvas.addEventListener("webglcontextlost", (e) => {
      e.preventDefault();
      console.warn("ThumbRenderer: WebGL context lost");
    });

    const stage=createScenePreviewStage(this.canvas,true);
    this.renderer=stage.renderer;this.scene=stage.scene;this.camera=stage.camera;
    this.renderer.setSize(SIZE, SIZE, false);
  }

  /** Render one asset to PNG bytes. Disposes the loaded subject before returning. */
  async render(asset: Asset): Promise<Uint8Array> {
    return this.renderPrepared(await this.prepare(asset));
  }

  /** Scene loading can overlap other thumbnails; GPU capture stays serial. */
  async prepare(asset: Asset): Promise<PreparedSubject> {
    if(asset.kind==="audio")return {png:await audioThumbnail(asset)};
    // A plugin-defined type draws its own picture; the host only stores it.
    if(asset.kind==="plugin") {
      // The catalog can load before plugin discovery finishes.
      await assetTypesLoaded();
      const render=assetTypeEntry(asset.type).implementation?.renderThumbnail;
      if(!render)throw new Error(`No installed plugin renders ${asset.type} previews`);
      const png=await render(asset,pluginAssetFiles,SIZE,this.tools(asset.library));
      if(!(png instanceof Uint8Array)||!png.length)throw new Error(`The ${asset.type} plugin returned no preview image`);
      return {png};
    }
    this.initialize();
    return loadThumbSubject(asset, this.renderer);
  }

  discard(subject: PreparedSubject): void {
    if(!("png" in subject))disposeScene(subject);
  }

  async renderPrepared(subject: PreparedSubject, size = SIZE): Promise<Uint8Array> {
    if("png" in subject)return subject.png;
    this.initialize();
    // Render and capture without yielding: a plugin's picture can be drawn while another asset is being
    // prepared, and the shared scene must hold only this subject while it is drawn. `toBlob` copies the
    // canvas when it is called, so the subject can go before the PNG is encoded.
    let capture: Promise<Uint8Array>;
    try {
      if(size!==SIZE)this.renderer.setSize(size,size,false);
      this.scene.add(subject);
      fitCameraToObject(this.camera, subject);
      this.renderer.render(this.scene, this.camera);
      capture=capturePng(this.renderer);
    } finally {
      if(size!==SIZE)this.renderer.setSize(SIZE,SIZE,false);
      this.scene.remove(subject);
      disposeScene(subject);
    }
    return capture;
  }

  /** What a plugin's `renderThumbnail` may ask of the host, for an asset of `library`: material pictures drawn
   *  exactly as the library's own material thumbnails are, on this renderer. */
  private tools(library: string): ThumbnailTools {
    return {
      materialSphere: async (input, size) => {
        this.initialize();
        const source = { maps: input.maps.map((map) => ({ ...map, invert: !!map.invert })), flipNormalGreen: !!input.flipNormalGreen, resolveUrl: async (file: string) => input.resolveUrl(file) };
        return this.renderPrepared(await materialSubject(source, this.renderer), size);
      },
      assetThumbnail: async (id, size) => {
        try {
          const asset = await fetchAsset(parseQualified(id) ? id : qualify(library, id));
          if (asset?.kind !== "material") return undefined;
          this.initialize();
          return await this.renderPrepared(await materialSubject(materialSourceOf(asset), this.renderer), size);
        } catch {
          return undefined;
        }
      },
      derivedAssets: async (id) => {
        const found = await derivedAssets(parseQualified(id) ? id : qualify(library, id));
        cacheAssets(found);
        return found;
      },
    };
  }

  dispose(): void {
    this.renderer?.dispose();
    this.renderer?.forceContextLoss();
    this.canvas?.remove();
  }
}

/** Capture the renderer's canvas as PNG bytes, preferring toBlob over dataURL. */
async function capturePng(renderer: WebGLRenderer): Promise<Uint8Array> {
  const canvas = renderer.domElement as HTMLCanvasElement;
  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, "image/png"),
  );
  if (blob) return new Uint8Array(await blob.arrayBuffer());

  // Fallback: data URL -> bytes.
  const dataUrl = canvas.toDataURL("image/png");
  return dataUrlToBytes(dataUrl);
}

function dataUrlToBytes(dataUrl: string): Uint8Array {
  const base64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
