import test from "node:test";
import assert from "node:assert/strict";
import { variantInfo, groupSeedVariants } from "../packages/toybox-sdk/dist/variants.js";

test("model detail levels group without treating colours as variants", () => {
  const entry = (file) => ({ id: `uid://import_p_props_${file.replace('.gltf', '')}`, pack: "p", category: "props", file });
  const grouped = groupSeedVariants([entry("Tree_LOD2.gltf"), entry("Tree_LOD0.gltf"), entry("Tree_LOD1.gltf"), entry("Tree_Red.gltf")]);
  assert.equal(grouped.length, 2);
  assert.equal(grouped[0].id, "uid://import_p_props_Tree");
  assert.equal(grouped[0].file, "Tree_LOD0.gltf");
  assert.deepEqual(grouped[0].variants.map((v) => v.file), ["Tree_LOD1.gltf", "Tree_LOD2.gltf"]);
  assert.equal(grouped[1].file, "Tree_Red.gltf");
});

test("material sizes sort largest first and preserve each original fileset", () => {
  const entry = (name) => ({ kind: "material", id: `uid://import_p_material_${name}`, pack: "p", category: "wood", name, maps: [{ role: "albedo", file: `${name}.png` }] });
  const [material] = groupSeedVariants([entry("Wood_1K"), entry("Wood_4096"), entry("Wood_2K")]);
  assert.equal(material.id, "uid://import_p_material_Wood");
  assert.equal(material.name, "Wood_4096");
  assert.deepEqual(material.variants.map((v) => v.maps[0].file), ["Wood_2K.png", "Wood_1K.png"]);
});

test("reimport of an asset without variants keeps its existing id", () => {
  const model = { id: "uid://import_p_props_Chair", pack: "p", category: "props", file: "Chair.gltf" };
  const material = { kind: "material", id: "uid://import_p_material_Stone", pack: "p", category: "stone", name: "Stone", maps: [] };
  assert.deepEqual(groupSeedVariants([model, material]), [model, material]);
});

test("packs, categories and kinds remain separate", () => {
  const entries = ["a", "b"].map((pack) => ({ id: pack, pack, category: "props", file: "Tree_LOD0.gltf" }));
  assert.equal(groupSeedVariants(entries).length, 2);
  assert.equal(variantInfo("Tree_Blue", "model").base, "Tree_Blue");
  assert.equal(variantInfo("Tree_LOD1.gltf", "model").label, "Medium");
  assert.equal(variantInfo("Wood_2K", "material").label, "2048");
});

test("animation source names are not interpreted as detail or texture-size variants", () => {
  const sources = ["Walk_LOD1.gltf", "Jump_2K.gltf"].map((file) => ({ kind: "animation", id: file, file, pack: "p", category: "animations" }));
  assert.deepEqual(groupSeedVariants(sources), sources);
  assert.equal(variantInfo("Walk_2K", "animation").base, "Walk_2K");
});

test("a recorded import format survives variant grouping on the group and on each variant", () => {
  const entry = (file, format) => ({ id: `uid://import_p_props_${file}`, pack: "p", category: "props", file, format });
  const [group] = groupSeedVariants([entry("Tree_LOD1.gltf", "obj"), entry("Tree_LOD0.gltf", "fbx")]);
  assert.equal(group.format, "fbx");
  assert.deepEqual(group.variants.map((v) => v.format), ["obj"]);
});
