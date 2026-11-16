import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const source = readFileSync(new URL("../src/domain/gltfResource.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } });
const { gltfResourcePath } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
test("relative resources retain directory identity rather than matching atlas basenames", () => {
  assert.equal(gltfResourcePath("library/p/characters/Hero.gltf", "../textures/Atlas.png"), "library/p/textures/Atlas.png");
  assert.equal(gltfResourcePath("library/p/characters/Hero.gltf", "../buildings/Atlas.png"), "library/p/buildings/Atlas.png");
  assert.equal(gltfResourcePath("library\\p\\Hero.gltf", "textures/My%20Atlas.png"), "library/p/textures/My Atlas.png");
  assert.equal(gltfResourcePath("a.gltf", "../../outside.bin"), null);
  assert.equal(gltfResourcePath("a.gltf", "data:image/png;base64,AA"), null);
  assert.equal(gltfResourcePath("a.gltf", "blob:example"), null);
});
