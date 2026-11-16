// Restore omitted SciFi Space atlas bindings from the pack's MaterialList.
// Dry-run by default. Never guesses an atlas from a filename or alters geometry.
import { readFile, writeFile, mkdir, rename } from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";

const [libraryArg, sourceArg, ...flags] = process.argv.slice(2);
if (!libraryArg || !sourceArg || flags.some(flag => flag !== "--apply")) {
  throw Error("Usage: node scripts/repair-synty-bindings.mjs <library-root> <source-pack-root> [--apply]");
}
const apply = flags.includes("--apply");
const root = path.resolve(libraryArg);
const source = path.resolve(sourceArg);
const pack = "polygon_scifi_space";
const materialList = path.join(source, "MaterialList_PolygonSciFiSpace.txt");
const text = await readFile(materialList, "utf8");
const bindings = new Map();
let mesh;
for (const line of text.split(/\r?\n/)) {
  const name = line.match(/^\s*Mesh Name:\s*(.+?)\s*$/);
  if (name) { mesh = name[1]; continue; }
  const slot = line.match(/^\s*Slot:\s*(.+?)\s+\((.+?)\)\s*$/);
  if (!slot || !mesh) continue;
  if (!bindings.has(mesh)) bindings.set(mesh, new Map());
  bindings.get(mesh).set(`${slot[1]}|${slot[2]}`, { material: slot[1], texture: slot[2] });
}
const stamp = Date.now();
const backupRoot = path.join(root, "_library_config", `synty-binding-backup-${stamp}`);
const manifest = JSON.parse(await readFile(path.join(root, "_library_config/catalog.json"), "utf8"));
const report = { source: materialList, changed: [], unresolved: [], backup: null };
const digest = bytes => createHash("sha256").update(bytes).digest("hex");
for (const entry of manifest.assets.filter(a => a.pack === pack && a.category === "characters" && (a.kind ?? "model") === "model")) {
  const relative = `library/${pack}/characters/${entry.file}`;
  const file = path.resolve(root, relative);
  if (path.dirname(file) !== path.join(root, "library", pack, "characters")) throw Error("Scene path escapes character folder");
  const original = await readFile(file);
  const doc = JSON.parse(original);
  let modified = false;
  const added = new Map();
  for (const gltfMesh of doc.meshes ?? []) {
    for (const primitive of gltfMesh.primitives ?? []) {
      const material = doc.materials?.[primitive.material];
      if (!material || material.pbrMetallicRoughness?.baseColorTexture) continue;
      // Only the known placeholder from this legacy FBX conversion. Glass,
      // vertex-painted heads, and already textured materials are left alone.
      if (!/^SciFiSpaceBase\d*$/.test(material.name ?? "") || material.extras?.fromFBX?.isTruePBR !== false) continue;
      const meshName = (gltfMesh.name ?? "").replace(/^SK_/, "SM_");
      const options = [...(bindings.get(meshName)?.values() ?? [])];
      if (options.length !== 1 || options[0].texture !== "PolygonSciFiSpace_Texture_01_A" || primitive.attributes?.TEXCOORD_0 === undefined) {
        report.unresolved.push({ file: relative, mesh: gltfMesh.name }); continue;
      }
      const binding = options[0];
      const key = `${primitive.material}|${binding.material}|${binding.texture}`;
      if (!added.has(key)) {
        const filename = `${binding.texture}.png`;
        const sourceTexture = path.join(source, "Textures", filename);
        const libraryTexture = path.join(root, "library", pack, "textures", filename);
        const sourceBytes = await readFile(sourceTexture);
        if (digest(sourceBytes) !== digest(await readFile(libraryTexture))) throw Error("Library atlas differs from source pack");
        doc.images ??= []; doc.textures ??= [];
        const uri = `../textures/${filename}`;
        let image = doc.images.findIndex(image => image.uri === uri);
        if (image < 0) image = doc.images.push({ uri }) - 1;
        let texture = doc.textures.findIndex(texture => texture.source === image);
        if (texture < 0) texture = doc.textures.push({ source: image }) - 1;
        const patched = structuredClone(material);
        patched.name = binding.material;
        patched.pbrMetallicRoughness ??= {};
        patched.pbrMetallicRoughness.baseColorTexture = { index: texture, texCoord: 0 };
        patched.pbrMetallicRoughness.baseColorFactor = [1, 1, 1, 1];
        added.set(key, doc.materials.push(patched) - 1);
      }
      primitive.material = added.get(key);
      modified = true;
    }
  }
  if (!modified) continue;
  report.changed.push(relative);
  if (apply) {
    const backup = path.join(backupRoot, relative);
    await mkdir(path.dirname(backup), { recursive: true });
    await writeFile(backup, original, { flag: "wx" });
    if (digest(await readFile(file)) !== digest(original)) throw Error("Scene changed during repair");
    const temp = `${file}.${stamp}.tmp`;
    await writeFile(temp, JSON.stringify(doc, null, 2) + "\n", { flag: "wx" });
    await rename(temp, file);
    report.backup = backupRoot;
  }
}
if (apply && report.changed.length) {
  await writeFile(path.join(backupRoot, "report.json"), JSON.stringify(report, null, 2));
}
console.log(JSON.stringify(report, null, 2));
