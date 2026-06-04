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
  Color,
  DirectionalLight,
  HemisphereLight,
  PerspectiveCamera,
  Scene,
  WebGLRenderer,
} from "three";

import type { Asset } from "../domain/catalog";
import { loadAssetScene } from "./gltfLoad";
import { disposeScene } from "./disposeScene";
import { fitCameraToObject } from "./fitCamera";

const SIZE = 256;

export class ThumbRenderer {
  private readonly renderer: WebGLRenderer;
  private readonly scene: Scene;
  private readonly camera: PerspectiveCamera;

  private readonly canvas: HTMLCanvasElement;

  constructor() {
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

    this.renderer = new WebGLRenderer({
      canvas: this.canvas,
      antialias: true,
      alpha: true,
      preserveDrawingBuffer: true, // required for reliable canvas capture
    });
    this.renderer.setSize(SIZE, SIZE, false);
    this.renderer.setClearColor(new Color(0x14161a), 1);

    this.scene = new Scene();
    this.scene.add(new HemisphereLight(0xcfd8e3, 0x2b2f38, 0.9));
    const key = new DirectionalLight(0xffffff, 1.2);
    key.position.set(5, 8, 5);
    const fill = new DirectionalLight(0xffffff, 0.4);
    fill.position.set(-4, 2, -3);
    this.scene.add(key, fill);

    this.camera = new PerspectiveCamera(45, 1, 0.01, 1000);
  }

  /** Render one asset to PNG bytes. Disposes the loaded model before returning. */
  async render(asset: Asset): Promise<Uint8Array> {
    // Thumbnails are bind-pose stills; animation clips are ignored.
    const { scene: group } = await loadAssetScene(asset);
    try {
      this.scene.add(group);
      fitCameraToObject(this.camera, group);
      this.renderer.render(this.scene, this.camera);
      return await capturePng(this.renderer);
    } finally {
      this.scene.remove(group);
      disposeScene(group);
    }
  }

  dispose(): void {
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    this.canvas.remove();
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
