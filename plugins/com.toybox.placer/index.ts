/**
 * Godot asset_placer exporter — the reference plugin.
 *
 * Reproduces what the app's old native `export_placer` command did, now as a
 * thin orchestration over the host primitives: per asset, bake a self-contained
 * `.glb` (or write a loose `.gltf` copy), then merge the Godot
 * `asset_library.json` so the addon's dock discovers the published assets.
 * Toybox's facets (pack, category, favorite, user-tags) become the addon's
 * collections — that banding/merge logic stays in Rust behind `host.placerMerge`.
 *
 * Authored against the `toybox` SDK, resolved at runtime through the app's
 * shared import map.
 */

import { defineExporter, type AssetView, type PlacerAssetInput } from "@ldlework/toybox-sdk";

interface Written {
  resPath: string;
  name: string;
}

export default defineExporter({
  async run(ctx, assets) {
    const { host, fs, config, report } = ctx;
    const format = (config.format as "glb" | "copy") ?? "glb";
    const preserve = config.preserveStructure;
    const targetDir = config.targetDir;
    const subDir = trimSlashes(String(config.subDir ?? ""));
    const libraryJson = String(config.libraryJson ?? "");

    const usedStems = new Set<string>();
    const placerAssets: PlacerAssetInput[] = [];

    for (const asset of assets) {
      const stem = resolveStem(asset, preserve, usedStems, report);
      const written =
        format === "glb"
          ? await writeGlb(ctx, asset, stem, preserve, targetDir, subDir)
          : await writeCopy(ctx, asset, stem, preserve, targetDir, subDir);
      placerAssets.push(toPlacerAsset(asset, written));
    }

    await host.placerMerge(libraryJson, resUri(subDir), placerAssets);
    return report.done();

    // --- per-asset writers ---------------------------------------------------

    async function writeGlb(
      c: typeof ctx,
      asset: AssetView,
      stem: string,
      preserveStructure: boolean,
      target: string,
      sub: string,
    ): Promise<Written> {
      const glb = await c.host.assembleGlb(asset.id);
      const rel = layoutRel(preserveStructure, asset, `${stem}.glb`);
      const full = joinRel(sub, rel);
      await c.fs.writeBytes(target, joinRel(sub, rel), glb);
      report.write(full);
      return { resPath: resUri(full), name: `${stem}.glb` };
    }

    async function writeCopy(
      c: typeof ctx,
      asset: AssetView,
      stem: string,
      preserveStructure: boolean,
      target: string,
      sub: string,
    ): Promise<Written> {
      // The host copy primitive writes under <target>/<sub> via the same
      // plan_copy path the native copy export uses; it returns the sub-relative
      // paths it wrote, which we re-root under sub for the report + res id.
      const subTarget = sub ? `${target}/${sub}` : target;
      const written = await c.host.performCopy(asset.id, subTarget, stem, preserveStructure);
      for (const w of written) report.write(joinRel(sub, w));
      const gltfRel = layoutRel(preserveStructure, asset, `${stem}.gltf`);
      const full = joinRel(sub, gltfRel);
      return { resPath: resUri(full), name: `${stem}.gltf` };
    }
  },
});

/** Flatten-only collision guard: namespace a duplicate stem with the pack.
 *  (Preserve mode can't collide — the pack/category dirs disambiguate.) */
function resolveStem(
  asset: AssetView,
  preserve: boolean,
  used: Set<string>,
  report: { warn(m: string): void },
): string {
  if (preserve || !used.has(asset.name)) {
    used.add(asset.name);
    return asset.name;
  }
  const namespaced = `${asset.pack}_${asset.name}`;
  report.warn(`renamed '${asset.name}' -> '${namespaced}' to avoid a flattened name collision`);
  used.add(namespaced);
  return namespaced;
}

function toPlacerAsset(asset: AssetView, written: Written): PlacerAssetInput {
  return {
    pack: asset.pack,
    category: asset.category,
    favorite: asset.user.favorite,
    tags: asset.user.tags,
    resPath: written.resPath,
    name: written.name,
  };
}

/** Target-relative path of an asset file given the layout. */
function layoutRel(preserve: boolean, asset: AssetView, file: string): string {
  return preserve ? `${asset.pack}/${asset.category}/${file}` : file;
}

/** Join a project-relative sub_dir and an asset-relative path with `/`. */
function joinRel(subDir: string, rel: string): string {
  const norm = rel.replace(/\\/g, "/");
  return subDir ? `${subDir}/${norm}` : norm;
}

/** Form a `res://...`-style id from a project-relative path. */
function resUri(projectRel: string): string {
  return `res://${projectRel.replace(/^\/+/, "")}`;
}

function trimSlashes(s: string): string {
  return s.replace(/^[/\\]+|[/\\]+$/g, "").replace(/\\/g, "/");
}
