/**
 * A procedural studio environment map for image-based lighting. Metals render
 * black without an environment to reflect, so material previews need one. Built
 * from three's `RoomEnvironment` (no network, no HDR file), baked once and
 * shared across renderers. Never dispose the cached texture with a material.
 */

import { type Texture, type WebGLRenderer, PMREMGenerator, DataTexture, RGBAFormat, HalfFloatType, LinearFilter } from "three";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";

const cache = new WeakMap<WebGLRenderer, Texture>();
// CPU-backed cube-UV data can be uploaded into any renderer. A render-target
// texture cannot: previously each newly opened detail view baked the room again.
let sharedEnvironment:Texture|undefined;

/** The renderer's shared studio environment (PMREM-filtered), created on first use. */
export function studioEnvironment(renderer: WebGLRenderer): Texture {
  if(sharedEnvironment)return sharedEnvironment;
  let env = cache.get(renderer);
  if (!env) {
    env = generate(renderer);
    cache.set(renderer, env);
  }
  return env;
}

function generate(renderer: WebGLRenderer): Texture {
  const pmrem = new PMREMGenerator(renderer);
  const room = new RoomEnvironment();
  try {
    const target=pmrem.fromScene(room,0.04);
    try {
      const data=new Uint16Array(target.width*target.height*4);
      renderer.readRenderTargetPixels(target,0,0,target.width,target.height,data);
      if(data.some((value,index)=>index%4!==3&&value!==0)) {
        const texture=new DataTexture(data,target.width,target.height,RGBAFormat,HalfFloatType);
        texture.mapping=target.texture.mapping;texture.colorSpace=target.texture.colorSpace;
        texture.minFilter=LinearFilter;texture.magFilter=LinearFilter;texture.generateMipmaps=false;
        texture.needsUpdate=true;sharedEnvironment=texture;target.dispose();
        return texture;
      }
    }catch {
      // Retain the generated GPU map on devices without half-float readback.
    }
    return target.texture;
  } finally {
    room.dispose();
    pmrem.dispose();
  }
}
