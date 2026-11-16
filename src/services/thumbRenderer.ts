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

import type { Asset } from "../domain/catalog";
import { disposeScene } from "./disposeScene";
import { fitCameraToObject } from "./fitCamera";
import { loadThumbSubject } from "./thumbSubject";
import { audioThumbnail } from "./audioThumbnail";
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
    this.initialize();
    return loadThumbSubject(asset, this.renderer);
  }

  discard(subject: PreparedSubject): void {
    if(!("png" in subject))disposeScene(subject);
  }

  async renderPrepared(subject: PreparedSubject): Promise<Uint8Array> {
    if("png" in subject)return subject.png;
    this.initialize();
    try {
      this.scene.add(subject);
      fitCameraToObject(this.camera, subject);
      this.renderer.render(this.scene, this.camera);
      return await capturePng(this.renderer);
    } finally {
      this.scene.remove(subject);
      disposeScene(subject);
    }
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
