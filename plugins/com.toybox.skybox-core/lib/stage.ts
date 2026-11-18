/**
 * The scene a sky is shown in: the sky as background and lighting, a camera
 * that looks around from the centre, and optional reference spheres that show
 * what the sky does to a mirror and to a matte surface.
 */

import {
  ACESFilmicToneMapping, Group, Mesh, MeshStandardMaterial, NoToneMapping, PerspectiveCamera, Scene, SphereGeometry,
  SRGBColorSpace, WebGLRenderer,
} from "three";

import type { LoadedSky } from "./sky.js";

const MIN_FOV = 25, MAX_FOV = 110;
const MAX_PITCH = Math.PI / 2 - 0.01;

export class SkyStage {
  readonly renderer: WebGLRenderer;
  private scene = new Scene();
  private camera = new PerspectiveCamera(75, 1, 0.1, 100);
  private spheres = new Group();
  private sky?: LoadedSky;
  private yaw = 0;
  private pitch = 0;

  constructor(canvas: HTMLCanvasElement, options: { preserveDrawingBuffer?: boolean } = {}) {
    this.renderer = new WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: options.preserveDrawingBuffer });
    this.renderer.outputColorSpace = SRGBColorSpace;
    const geometry = new SphereGeometry(0.62, 48, 32);
    const mirror = new Mesh(geometry, new MeshStandardMaterial({ color: 0xffffff, metalness: 1, roughness: 0.03 }));
    const matte = new Mesh(geometry, new MeshStandardMaterial({ color: 0xb8b8b8, metalness: 0, roughness: 1 }));
    mirror.position.set(-0.78, -0.35, -3.4);
    matte.position.set(0.78, -0.35, -3.4);
    this.spheres.add(mirror, matte);
    this.spheres.visible = false;
    // Riding on the camera keeps the spheres in view while the sky turns
    // around them; their reflections still follow the world.
    this.camera.add(this.spheres);
    this.scene.add(this.camera);
    this.camera.rotation.order = "YXZ";
  }

  /** Show `sky`. The stage takes ownership and disposes it when replaced. */
  setSky(sky: LoadedSky | undefined): void {
    this.sky?.dispose();
    this.sky = sky;
    this.scene.background = sky?.texture ?? null;
    this.scene.environment = sky?.texture ?? null;
    // Real light levels need compressing for a display; ordinary images are
    // already display-referred and are shown as they are.
    this.renderer.toneMapping = sky?.hdr ? ACESFilmicToneMapping : NoToneMapping;
  }

  /** Brightness in photographic stops; 0 shows the image as stored. */
  setExposure(stops: number): void {
    const gain = 2 ** stops;
    this.scene.backgroundIntensity = gain;
    this.scene.environmentIntensity = gain;
  }

  setSpheres(visible: boolean): void { this.spheres.visible = visible; }

  look(yaw: number, pitch: number): void {
    this.yaw = yaw;
    this.pitch = Math.max(-MAX_PITCH, Math.min(MAX_PITCH, pitch));
    this.camera.rotation.set(this.pitch, this.yaw, 0);
  }

  /** Turn by a pointer drag, in pixels. Dragging pulls the sky with the pointer. */
  drag(dx: number, dy: number, viewportHeight: number): void {
    const radiansPerPixel = (this.camera.fov * Math.PI) / 180 / Math.max(1, viewportHeight);
    this.look(this.yaw + dx * radiansPerPixel, this.pitch + dy * radiansPerPixel);
  }

  get fov(): number { return this.camera.fov; }
  setFov(fov: number): void {
    this.camera.fov = Math.max(MIN_FOV, Math.min(MAX_FOV, fov));
    this.camera.updateProjectionMatrix();
  }

  resize(width: number, height: number, pixelRatio = 1): void {
    this.renderer.setPixelRatio(pixelRatio);
    this.renderer.setSize(Math.max(1, width), Math.max(1, height), false);
    this.camera.aspect = Math.max(1, width) / Math.max(1, height);
    this.camera.updateProjectionMatrix();
  }

  render(): void { this.renderer.render(this.scene, this.camera); }

  dispose(): void {
    this.setSky(undefined);
    for (const child of this.spheres.children) { const mesh = child as Mesh; (mesh.material as MeshStandardMaterial).dispose(); }
    (this.spheres.children[0] as Mesh | undefined)?.geometry.dispose();
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}
