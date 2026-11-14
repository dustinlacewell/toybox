/**
 * A procedural studio environment map for image-based lighting. Metals render
 * black without an environment to reflect, so material previews need one. Built
 * from three's `RoomEnvironment` (no network, no HDR file) and cached per
 * renderer. The cached texture is shared: never dispose it with a material.
 */

import { type Texture, type WebGLRenderer, PMREMGenerator } from "three";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";

const cache = new WeakMap<WebGLRenderer, Texture>();

/** The renderer's shared studio environment (PMREM-filtered), created on first use. */
export function studioEnvironment(renderer: WebGLRenderer): Texture {
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
    return pmrem.fromScene(room, 0.04).texture;
  } finally {
    room.dispose();
    pmrem.dispose();
  }
}
