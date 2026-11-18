import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

function moduleUrl(file) {
  const { outputText } = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
}
const plugin = "../plugins/com.toybox.skybox-core/lib/";
const { FACES, faceOf, findSkies, layoutFromSize, faceCells, readMeta, describeSize, skyId, cleanName, encodingOf } = await import(moduleUrl(`${plugin}detect.ts`));
const { imageSize } = await import(moduleUrl(`${plugin}header.ts`));
const { flipRows, shrink, cutCell } = await import(moduleUrl(`${plugin}pixels.ts`));

test("cube faces are read from the naming conventions in common use", () => {
  const cases = {
    "sky_px.png": ["px", "sky"], "sky_NX.png": ["nx", "sky"], "night-posy.hdr": ["py", "night"], "night.negz.exr": ["nz", "night"],
    "Right.jpg": ["px", ""], "left.jpg": ["nx", ""], "sky top.png": ["py", "sky"], "sky_up.png": ["py", "sky"], "sky_down.png": ["ny", "sky"],
    "sky_bottom.png": ["ny", "sky"], "sky_front.png": ["pz", "sky"], "sky_bk.png": ["nz", "sky"], "sky_rt.png": ["px", "sky"], "sky_lf.png": ["nx", "sky"],
    "sky+x.png": ["px", "sky"], "sky-z.png": ["nz", "sky"], "sky_+y.png": ["py", "sky"],
  };
  for (const [file, [face, prefix]] of Object.entries(cases)) assert.deepEqual(faceOf(file), { face, prefix }, file);
  // A token must stand alone: these are ordinary names, not faces.
  for (const file of ["backdrop.png", "setup.png", "pinx.png", "sunset.hdr", "topology.png", "leftover.png"]) assert.equal(faceOf(file), undefined, file);
});

test("six face images become one cube map; everything else stays its own image", () => {
  const skies = findSkies([
    "night/px.png", "night/nx.png", "night/py.png", "night/ny.png", "night/pz.png", "night/nz.png",
    "day\\Harbour_right.JPG", "day/Harbour_left.JPG", "day/Harbour_up.JPG", "day/Harbour_down.JPG", "day/Harbour_front.JPG", "day/Harbour_back.JPG",
    "studio_4k.hdr", "render.exr", "notes.txt", "textures/wood_albedo.png", "pano/sunset_panorama.jpg",
    // Five faces, and a set with two tops, are not cube maps.
    "broken/a_px.png", "broken/a_nx.png", "broken/a_py.png", "broken/a_ny.png", "broken/a_pz.png",
    "dup/b_px.png", "dup/b_nx.png", "dup/b_up.png", "dup/b_top.png", "dup/b_ny.png", "dup/b_pz.png",
  ]);
  const cubes = skies.filter((sky) => sky.layout === "cubemap");
  assert.deepEqual(cubes.map((sky) => sky.name), ["Harbour", "night"]);
  assert.deepEqual(cubes[1].files, FACES.map((face) => `night/${face}.png`));
  assert.equal(cubes[0].faces.py, "day/Harbour_up.JPG");
  assert.equal(cubes[0].format, "JPG");
  const images = skies.filter((sky) => sky.layout === "image");
  assert.equal(images.length, 2 + 2 + 5 + 6);
  assert.ok(!skies.some((sky) => sky.files.includes("notes.txt")));
  // HDR images, cube maps and sky-named images are ticked; stray textures are not.
  const likely = Object.fromEntries(skies.map((sky) => [sky.files[0], sky.likely]));
  assert.equal(likely["studio_4k.hdr"], true); assert.equal(likely["render.exr"], true); assert.equal(likely["pano/sunset_panorama.jpg"], true);
  assert.equal(likely["textures/wood_albedo.png"], false); assert.equal(likely["broken/a_px.png"], false);
  // Library names are folders, so they are unique regardless of case.
  const names = skies.map((sky) => sky.name.toLowerCase());
  assert.equal(new Set(names).size, names.length);
});

test("same-named skies in different folders get distinct, safe names", () => {
  const skies = findSkies(["a/sky.hdr", "b/Sky.hdr", "c/sky.hdr"]);
  assert.deepEqual(skies.map((sky) => sky.name).sort(), ["Sky 2", "sky", "sky 3"].sort());
  assert.equal(cleanName('  a<b>:c/"d"|e?*. '), "a_b__c__d__e");
  assert.equal(cleanName("..."), "Sky");
  assert.equal(encodingOf("x.HDR"), "hdr"); assert.equal(encodingOf("x.jpeg"), "ldr"); assert.equal(encodingOf("x.tga"), undefined);
});

test("an image's shape decides whether it is a panorama, a cross or a strip", () => {
  assert.deepEqual(layoutFromSize(4096, 2048), { layout: "equirect", certain: true });
  assert.deepEqual(layoutFromSize(4096, 3072), { layout: "cross-horizontal", certain: true });
  assert.deepEqual(layoutFromSize(3072, 4096), { layout: "cross-vertical", certain: true });
  assert.deepEqual(layoutFromSize(6144, 1024), { layout: "strip-horizontal", certain: true });
  assert.deepEqual(layoutFromSize(1024, 6144), { layout: "strip-vertical", certain: true });
  // Unusual shapes are shown as a panorama, and flagged for the user to check.
  assert.deepEqual(layoutFromSize(1920, 1080), { layout: "equirect", certain: false });
  assert.deepEqual(layoutFromSize(0, 0), { layout: "equirect", certain: false });
});

test("every unfolded layout places each of the six faces exactly once", () => {
  for (const layout of ["cross-horizontal", "cross-vertical", "strip-horizontal", "strip-vertical"]) {
    const { columns, rows, cells } = faceCells(layout);
    assert.deepEqual(cells.map((cell) => cell.face).sort(), [...FACES].sort(), layout);
    assert.equal(new Set(cells.map((cell) => `${cell.column},${cell.row}`)).size, 6, layout);
    assert.ok(cells.every((cell) => cell.column < columns && cell.row < rows), layout);
  }
  assert.equal(faceCells("equirect"), undefined);
  // Only the vertical cross stores a face upside down.
  assert.deepEqual(faceCells("cross-vertical").cells.filter((cell) => cell.turn).map((cell) => cell.face), ["nz"]);
  assert.equal(faceCells("cross-horizontal").cells.some((cell) => cell.turn), false);
});

test("stored metadata is read defensively and described for people", () => {
  const cubeFiles = FACES.map((face) => `library/skybox/k/sky_${face}.png`);
  const faces = Object.fromEntries(FACES.map((face) => [face, `sky_${face}.png`]));
  const cube = readMeta({ layout: "cubemap", encoding: "ldr", format: "PNG", width: 1024, height: 1024, faces }, cubeFiles);
  assert.equal(cube.layout, "cubemap"); assert.equal(cube.faces.nz, "sky_nz.png"); assert.equal(describeSize(cube), "1024 × 1024 per face");
  // Missing metadata is rebuilt from the files themselves.
  const bare = readMeta(undefined, cubeFiles);
  assert.equal(bare.layout, "cubemap"); assert.deepEqual(bare.faces, faces); assert.equal(bare.format, "PNG"); assert.equal(describeSize(bare), "Unknown");
  // Faces that do not match the files are ignored instead of trusted.
  const wrong = readMeta({ layout: "cubemap", faces: { ...faces, px: "missing.png" }, width: 8, height: 8 }, ["library/skybox/k/pano.hdr"]);
  assert.equal(wrong.layout, "equirect"); assert.equal(wrong.encoding, "hdr"); assert.equal(wrong.faces, undefined);
  const cross = readMeta({ layout: "cross-horizontal", encoding: "exr", format: "EXR", width: 4096, height: 3072 }, ["library/skybox/k/c.exr"]);
  assert.equal(describeSize(cross), "1024 × 1024 per face");
  assert.equal(describeSize(readMeta({ layout: "nonsense", width: 4096, height: 2048 }, ["a.hdr"])), "4096 × 2048");
  assert.equal(readMeta("garbage", ["a.jpg"]).format, "JPG");
});

test("ids are stable per source and name, so importing again updates a sky", () => {
  assert.equal(skyId("_import_1", "Sunset"), skyId("_import_1", "sunset"));
  assert.notEqual(skyId("_import_1", "Sunset"), skyId("_import_2", "Sunset"));
  assert.match(skyId("_import_1", "Sunset"), /^uid:\/\/skybox_[0-9a-f]+$/);
});

const bytes = (...parts) => Uint8Array.from(parts.flatMap((part) => typeof part === "string" ? [...Buffer.from(part, "latin1")] : part));
const u32 = (value, little = false) => { const out = Buffer.alloc(4); little ? out.writeUInt32LE(value) : out.writeUInt32BE(value); return [...out]; };
const i32 = (value) => { const out = Buffer.alloc(4); out.writeInt32LE(value); return [...out]; };
const u16 = (value, little = false) => { const out = Buffer.alloc(2); little ? out.writeUInt16LE(value) : out.writeUInt16BE(value); return [...out]; };

test("image sizes are read from the header of every supported format", () => {
  assert.deepEqual(imageSize(bytes("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nEXPOSURE=1\n\n-Y 2048 +X 4096\n", [1, 2, 3])), { width: 4096, height: 2048 });
  assert.deepEqual(imageSize(bytes("#?RGBE\n\n+X 8 -Y 4\n")), { width: 8, height: 4 });
  assert.deepEqual(imageSize(bytes([0x89], "PNG\r\n\x1a\n", u32(13), "IHDR", u32(1024), u32(512), [8, 6, 0, 0, 0])), { width: 1024, height: 512 });
  const attribute = (name, type, data) => [...Buffer.from(`${name}\0${type}\0`, "latin1"), ...i32(data.length), ...data];
  const exr = bytes(u32(20000630, true), [2, 0, 0, 0], attribute("channels", "chlist", [0]), attribute("compression", "compression", [3]),
    attribute("dataWindow", "box2i", [...i32(0), ...i32(0), ...i32(8191), ...i32(4095)]), [0]);
  assert.deepEqual(imageSize(exr), { width: 8192, height: 4096 });
  const jpeg = bytes([0xff, 0xd8, 0xff, 0xe0], u16(16), "JFIF\0", new Array(9).fill(0), [0xff, 0xdb], u16(4), [0, 0], [0xff, 0xc2], u16(17), [8], u16(768), u16(1536), [3], new Array(9).fill(0));
  assert.deepEqual(imageSize(jpeg), { width: 1536, height: 768 });
  const webpX = bytes("RIFF", u32(100, true), "WEBP", "VP8X", u32(10, true), [0, 0, 0, 0], [0xff, 0x0f, 0x00], [0xff, 0x07, 0x00], new Array(8).fill(0));
  assert.deepEqual(imageSize(webpX), { width: 4096, height: 2048 });
  const lossless = bytes("RIFF", u32(100, true), "WEBP", "VP8L", u32(10, true), [0x2f], u32((2047) | (1023 << 14), true), new Array(8).fill(0));
  assert.deepEqual(imageSize(lossless), { width: 2048, height: 1024 });
});

test("unknown, empty and truncated files have no size instead of a wrong one", () => {
  for (const data of [new Uint8Array(0), bytes("not an image at all, just text"), bytes("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n"), bytes([0x89], "PNG\r\n\x1a\n"),
    bytes(u32(20000630, true), [2, 0, 0, 0], "dataWindow\0box2i\0"), bytes([0xff, 0xd8, 0xff, 0xe0], u16(4), [0, 0]), bytes("RIFF", u32(4, true), "WEBP")]) {
    assert.equal(imageSize(data), undefined);
  }
});

test("pixel grids flip, shrink and cut into faces without mixing pixels", () => {
  // A 4 × 2 image with 2 samples per pixel; each pixel is [x, y].
  const grid = { width: 4, height: 2, channels: 2, data: Float32Array.from([0, 0, 1, 0, 2, 0, 3, 0, 0, 1, 1, 1, 2, 1, 3, 1]) };
  const flipped = flipRows(grid);
  assert.deepEqual([...flipped.data], [0, 1, 1, 1, 2, 1, 3, 1, 0, 0, 1, 0, 2, 0, 3, 0]);
  assert.deepEqual([...flipRows(flipped).data], [...grid.data]);
  assert.ok(flipped.data instanceof Float32Array);
  const half = shrink(grid, 2);
  assert.deepEqual({ width: half.width, height: half.height, data: [...half.data] }, { width: 2, height: 1, data: [0, 0, 2, 0] });
  assert.equal(shrink(grid, 4), grid);
  assert.equal(shrink(grid, 100), grid);
  // Cut the right-hand 2 × 2 half, then the same half turned upside down.
  const cell = cutCell(grid, 2, 1, { face: "px", column: 1, row: 0 });
  assert.deepEqual({ width: cell.width, height: cell.height, data: [...cell.data] }, { width: 2, height: 2, data: [2, 0, 3, 0, 2, 1, 3, 1] });
  assert.deepEqual([...cutCell(grid, 2, 1, { face: "nz", column: 1, row: 0, turn: true }).data], [3, 1, 2, 1, 3, 0, 2, 0]);
  const halfFloat = cutCell({ width: 2, height: 2, channels: 1, data: Uint16Array.from([1, 2, 3, 4]) }, 2, 2, { face: "py", column: 1, row: 1 });
  assert.ok(halfFloat.data instanceof Uint16Array); assert.deepEqual([...halfFloat.data], [4]);
});
