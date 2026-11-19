// Each importer plugin's command-line module, loaded from its compiled dist as
// `tbx` loads it, run end to end against a fake host backed by real temp folders.
// Run `tsc -p plugins/<id>/tsconfig.json` (cores first) before this file.

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";

import { listTree, loadPlugins, tempDir, writeTree } from "./support/cli-host.mjs";

const bytes = (...values) => new Uint8Array(values);
const library = () => tempDir("library");

/** A minimal glTF with one buffer file beside it, optionally with an animation. */
function gltf(animated = false) {
  return JSON.stringify({
    asset: { version: "2.0" },
    buffers: [{ uri: "geometry.bin", byteLength: 4 }],
    ...(animated ? { animations: [{ name: "walk", channels: [{ sampler: 0, target: { node: 0, path: "translation" } }], samplers: [] }] } : {}),
  });
}

/** A Radiance .hdr header for a width x height panorama. */
const hdr = (width, height) => new TextEncoder().encode(`#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y ${height} +X ${width}\n`);

// --- audio ---------------------------------------------------------------------

test("tbx import audio stages the sounds of a folder and commits one entry each", async () => {
  const src = writeTree(tempDir("audio-src"), {
    "step.wav": bytes(1, 2, 3), "rooms/wind.ogg": bytes(4, 5), "rooms/deep/boom.FLAC": bytes(6), "notes.txt": "not a sound", "cover.png": bytes(9),
  });
  const root = library();
  const host = await loadPlugins(["com.toybox.audio-core"], { libraryRoot: root });
  const result = await host.runImporter("com.toybox.audio-core", "audio", [src], { category: "sfx" });

  assert.equal(host.committed.length, 3);
  assert.deepEqual(host.committed.map((entry) => entry.file).sort(), ["rooms/deep/boom.FLAC", "rooms/wind.ogg", "step.wav"]);
  const [first] = host.committed;
  assert.equal(first.kind, "audio");
  assert.equal(first.category, "sfx");
  assert.match(first.pack, /^_import_/);
  assert.deepEqual(listTree(root), [`library/${first.pack}/sfx/rooms/deep/boom.FLAC`, `library/${first.pack}/sfx/rooms/wind.ogg`, `library/${first.pack}/sfx/step.wav`]);
  assert.deepEqual([...readFileSync(path.join(root, "library", first.pack, "sfx", "step.wav"))], [1, 2, 3]);
  assert.deepEqual(host.committed.find((entry) => entry.file === "rooms/deep/boom.FLAC").format, "flac");
  assert.deepEqual(result.imported.slice().sort(), host.committed.map((entry) => entry.id).sort());
});

test("tbx import audio takes single files, skips what is not audio and uses sounds as the default category", async () => {
  const src = writeTree(tempDir("audio-src"), { "a.mp3": bytes(1), "b.txt": "x" });
  const root = library();
  const host = await loadPlugins(["com.toybox.audio-core"], { libraryRoot: root });
  const result = await host.runImporter("com.toybox.audio-core", "audio", [path.join(src, "a.mp3"), path.join(src, "b.txt")]);
  assert.equal(host.committed.length, 1);
  assert.equal(host.committed[0].category, "sounds");
  assert.equal(host.committed[0].file, "a.mp3");
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /b\.txt/);
});

test("tbx import audio fails clearly when there is nothing to import, or the category or path is wrong", async () => {
  const src = writeTree(tempDir("audio-src"), { "readme.txt": "none here" });
  const host = await loadPlugins(["com.toybox.audio-core"], { libraryRoot: library() });
  await assert.rejects(host.runImporter("com.toybox.audio-core", "audio", [src]), /No WAV, MP3, Ogg Vorbis or FLAC files found/);
  await assert.rejects(host.runImporter("com.toybox.audio-core", "audio", [path.join(src, "missing")]), /does not exist/);
  writeTree(src, { "x.wav": bytes(1) });
  await assert.rejects(host.runImporter("com.toybox.audio-core", "audio", [src], { category: "a/b" }), /category/);
  assert.equal(host.committed.length, 0);
});

// --- materials -----------------------------------------------------------------

test("tbx import material groups a texture folder into materials", async () => {
  const src = writeTree(tempDir("material-src"), {
    "Wood/Wood_albedo.png": bytes(1), "Wood/Wood_normal.png": bytes(2), "Wood/Wood_roughness.png": bytes(3), "Wood/notes.txt": "x",
    "Stone/Stone_basecolor.jpg": bytes(4), "Stone/Stone_orm.png": bytes(5),
    "Bare/Bare_roughness.png": bytes(6),
  });
  const root = library();
  const host = await loadPlugins(["com.toybox.material-core"], { libraryRoot: root });
  const result = await host.runImporter("com.toybox.material-core", "material", [src], { category: "ground" });

  const names = host.committed.map((entry) => entry.name).sort();
  assert.deepEqual(names, ["Stone", "Wood"]);
  const wood = host.committed.find((entry) => entry.name === "Wood");
  assert.equal(wood.kind, "material");
  assert.equal(wood.category, "ground");
  assert.deepEqual(wood.maps.map((map) => map.role).sort(), ["albedo", "normal", "roughness"]);
  const stone = host.committed.find((entry) => entry.name === "Stone");
  assert.deepEqual(stone.maps.map((map) => [map.role, map.file, map.channel]).sort(), [
    ["albedo", "Stone_basecolor.jpg", undefined], ["ao", "Stone_orm.png", "r"], ["metallic", "Stone_orm.png", "b"], ["roughness", "Stone_orm.png", "g"],
  ]);
  assert.deepEqual(listTree(root).map((file) => file.split("/").slice(2).join("/")), [
    "materials/Stone/Stone_basecolor.jpg", "materials/Stone/Stone_orm.png", "materials/Wood/Wood_albedo.png", "materials/Wood/Wood_normal.png", "materials/Wood/Wood_roughness.png",
  ]);
  // A material with no colour map is reported, not imported.
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /Bare.*no albedo map/);
  assert.equal(result.imported.length, 2);
});

test("tbx import material groups named files from one folder together and fails when nothing is a material", async () => {
  const src = writeTree(tempDir("material-src"), { "Brick_albedo.png": bytes(1), "Brick_normal.png": bytes(2), "Brick_height.png": bytes(3), "lone_roughness.png": bytes(4) });
  const host = await loadPlugins(["com.toybox.material-core"], { libraryRoot: library() });
  await host.runImporter("com.toybox.material-core", "material", [path.join(src, "Brick_albedo.png"), path.join(src, "Brick_normal.png")]);
  assert.equal(host.committed.length, 1);
  assert.deepEqual(host.committed[0].maps.map((map) => map.role).sort(), ["albedo", "normal"]);
  assert.equal(host.committed[0].category, "uncategorized");
  await assert.rejects(host.runImporter("com.toybox.material-core", "material", [path.join(src, "lone_roughness.png")]), /No materials found/);
  await assert.rejects(host.runImporter("com.toybox.material-core", "material", [tempDir("empty")]), /No materials found/);
});

// --- models and animations -----------------------------------------------------

const modelPlugins = ["com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.animation-core"];

test("tbx import model converts glTF files through the format plugin's own command-line module", async () => {
  const src = writeTree(tempDir("model-src"), {
    "props/crate.gltf": gltf(), "props/geometry.bin": bytes(1, 2, 3, 4), "props/readme.md": "x",
    "props/barrel_LOD0.gltf": gltf(), "props/barrel_LOD1.gltf": gltf(),
  });
  const root = library();
  const host = await loadPlugins(modelPlugins, { libraryRoot: root });
  const result = await host.runImporter("com.toybox.model-core", "model", [src], { category: "props" });

  const files = host.committed.map((entry) => entry.file).sort();
  assert.ok(files.includes("crate.gltf"));
  const crate = host.committed.find((entry) => entry.file === "crate.gltf");
  assert.equal(crate.format, "gltf");
  assert.equal(crate.origin.plugin, "com.toybox.model-gltf");
  const staged = listTree(root);
  assert.ok(staged.some((file) => file.endsWith("/props/crate.gltf")));
  assert.ok(staged.some((file) => file.endsWith("/props/crate.bin")));
  assert.equal(JSON.parse(readFileSync(path.join(root, staged.find((file) => file.endsWith("/props/crate.gltf"))), "utf8")).buffers[0].uri, "crate.bin");
  // The two detail levels of one model become one entry with a variant.
  const barrel = host.committed.filter((entry) => /barrel/i.test(entry.file) || (entry.variants ?? []).some((v) => /barrel/i.test(v.file)));
  assert.equal(barrel.length, 1);
  assert.equal(barrel[0].variants?.length, 1);
  assert.equal(result.imported.length, host.committed.length);
});

test("tbx import model --levels best keeps only the best level of each model, and a file path imports that file", async () => {
  const src = writeTree(tempDir("model-src"), { "barrel_LOD0.gltf": gltf(), "barrel_LOD1.gltf": gltf(), "geometry.bin": bytes(1, 2, 3, 4), "crate.gltf": gltf() });
  const host = await loadPlugins(modelPlugins, { libraryRoot: library() });
  await host.runImporter("com.toybox.model-core", "model", [src], { levels: "best" });
  assert.equal(host.committed.length, 2);
  assert.ok(host.committed.every((entry) => !entry.variants?.length));
  host.committed.length = 0;
  await host.runImporter("com.toybox.model-core", "model", [path.join(src, "crate.gltf")]);
  assert.deepEqual(host.committed.map((entry) => entry.file), ["crate.gltf"]);
});

test("the models and animations importers refuse a category that is not a folder name", async () => {
  const src = writeTree(tempDir("model-src"), { "crate.gltf": gltf(true), "geometry.bin": bytes(1, 2, 3, 4) });
  const host = await loadPlugins([...modelPlugins, "com.toybox.material-core"], { libraryRoot: library() });
  for (const category of ["a/b", "a\\b", "..", "x."]) {
    await assert.rejects(host.runImporter("com.toybox.model-core", "model", [src], { category }), /category/, category);
    await assert.rejects(host.runImporter("com.toybox.animation-core", "animation", [src], { category }), /category/, category);
    await assert.rejects(host.runImporter("com.toybox.material-core", "material", [src], { category }), /category/, category);
  }
});

test("tbx import model fails clearly with no format plugin, no models, or an unreadable file", async () => {
  const src = writeTree(tempDir("model-src"), { "a.txt": "x", "bad.gltf": "{not json" });
  const withoutFormat = await loadPlugins(["com.toybox.model-core"], { libraryRoot: library() });
  await assert.rejects(withoutFormat.runImporter("com.toybox.model-core", "model", [src]), /No model formats are turned on/);
  const host = await loadPlugins(modelPlugins, { libraryRoot: library() });
  await assert.rejects(host.runImporter("com.toybox.model-core", "model", [path.join(src, "a.txt")]), /no turned-on plugin reads this file/);
  await assert.rejects(host.runImporter("com.toybox.model-core", "model", [src]), /bad-gltf/);
  assert.equal(host.committed.length, 0);
});

test("tbx import animation keeps the model files that hold clips and skips the others", async () => {
  const src = writeTree(tempDir("anim-src"), { "walk.gltf": gltf(true), "still.gltf": gltf(false), "geometry.bin": bytes(1, 2, 3, 4) });
  const root = library();
  const host = await loadPlugins(modelPlugins, { libraryRoot: root });
  const result = await host.runImporter("com.toybox.animation-core", "animation", [src]);

  assert.equal(host.committed.length, 1);
  const [entry] = host.committed;
  assert.equal(entry.kind, "animation");
  assert.equal(entry.file, "walk.gltf");
  assert.equal(entry.category, "animations");
  assert.equal(entry.format, "gltf");
  assert.ok(listTree(root).some((file) => file.endsWith("/animations/walk.gltf")));
  assert.deepEqual(result.imported, [entry.id]);
  assert.equal(result.skipped.length, 1);
  assert.match(result.skipped[0], /No animation clips in still\.gltf/);
  await assert.rejects(host.runImporter("com.toybox.animation-core", "animation", [path.join(src, "still.gltf")]), /No animation clips found/);
});

test("tbx import animation needs the Models plugin's module", async () => {
  const host = await loadPlugins(["com.toybox.animation-core"], { libraryRoot: library() });
  await assert.rejects(host.runImporter("com.toybox.animation-core", "animation", [tempDir("x")]), /Models plugin is not available/);
});

// --- skyboxes ------------------------------------------------------------------

test("tbx import skybox imports the skies of a folder: panoramas by default, every image with --all", async () => {
  const src = writeTree(tempDir("sky-src"), {
    "clouds.hdr": hdr(4096, 2048),
    "night/px.hdr": hdr(512, 512), "night/nx.hdr": hdr(512, 512), "night/py.hdr": hdr(512, 512), "night/ny.hdr": hdr(512, 512), "night/pz.hdr": hdr(512, 512), "night/nz.hdr": hdr(512, 512),
  });
  const root = library();
  const host = await loadPlugins(["com.toybox.skybox-core"], { libraryRoot: root });
  const result = await host.runImporter("com.toybox.skybox-core", "skybox", [src]);

  assert.equal(host.committed.length, 2);
  const clouds = host.committed.find((entry) => entry.name === "clouds");
  assert.equal(clouds.kind, "plugin");
  assert.equal(clouds.type, "skybox");
  assert.equal(clouds.category, "skies");
  assert.deepEqual(clouds.files, ["clouds.hdr"]);
  assert.deepEqual([clouds.meta.layout, clouds.meta.width, clouds.meta.height, clouds.meta.encoding], ["equirect", 4096, 2048, "hdr"]);
  const cube = host.committed.find((entry) => entry.meta.layout === "cubemap");
  assert.equal(cube.files.length, 6);
  assert.equal(cube.meta.faces.px, "px.hdr");
  assert.ok(listTree(root).some((file) => file.endsWith(`/skybox/clouds/clouds.hdr`)));
  assert.equal(listTree(root).filter((file) => file.includes(`/skybox/${cube.name}/`)).length, 6);
  assert.equal(result.imported.length, 2);
});

/** A PNG header for a width x height image (the importer reads only the size). */
const png = (width, height) => Uint8Array.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, width >>> 24, (width >> 16) & 255, (width >> 8) & 255, width & 255, height >>> 24, (height >> 16) & 255, (height >> 8) & 255, height & 255, 8, 6, 0, 0, 0]);

test("tbx import skybox honours --layout and --category, and names images that are not skies", async () => {
  const src = writeTree(tempDir("sky-src"), { "photo.png": png(1024, 768) });
  const host = await loadPlugins(["com.toybox.skybox-core"], { libraryRoot: library() });
  // A folder holding one image that does not look like a sky imports nothing without --all.
  await assert.rejects(host.runImporter("com.toybox.skybox-core", "skybox", [src]), /none look like skies/);
  await host.runImporter("com.toybox.skybox-core", "skybox", [src], { all: true, category: "backdrops", layout: "equirect" });
  assert.equal(host.committed.length, 1);
  assert.equal(host.committed[0].category, "backdrops");
  assert.equal(host.committed[0].meta.layout, "equirect");
  // Named on the command line, the same image is taken as it is, and a 4:3 image is read as a cross.
  host.committed.length = 0;
  await host.runImporter("com.toybox.skybox-core", "skybox", [path.join(src, "photo.png")]);
  assert.equal(host.committed[0].category, "skies");
  assert.equal(host.committed[0].meta.layout, "cross-horizontal");
  await assert.rejects(host.runImporter("com.toybox.skybox-core", "skybox", [src], { layout: "sideways" }), /Unknown layout/);
});

test("tbx import skybox reports an unreadable image instead of importing it", async () => {
  const src = writeTree(tempDir("sky-src"), { "broken.hdr": bytes(1, 2, 3), "fine.hdr": hdr(64, 32) });
  const host = await loadPlugins(["com.toybox.skybox-core"], { libraryRoot: library() });
  const result = await host.runImporter("com.toybox.skybox-core", "skybox", [src]);
  assert.deepEqual(host.committed.map((entry) => entry.name), ["fine"]);
  assert.match(result.skipped[0], /broken.*not a readable HDR image/);
  await assert.rejects(host.runImporter("com.toybox.skybox-core", "skybox", [path.join(src, "broken.hdr")]), /No skies found/);
});
