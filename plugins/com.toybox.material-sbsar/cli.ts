/**
 * The Substance plugin on the command line:
 *
 * - `tbx import sbsar` adds archives as substances and renders their materials;
 * - `tbx sbsar inputs|render|show|engine` look inside archives, render stored
 *   materials (or substances) again, show how they were made and check the engine.
 *
 * No user interface: the entry module (`index.ts`) draws the substance viewer and
 * thumbnails, so the command line has this module of its own and never reaches it.
 * The work is in `lib/`, shared with the panels and the viewer.
 */

import { definePlugin, runImport, type AssetView, type CommandRun, type ImportRun } from "@ldlework/toybox-sdk";

import { cleanupRenders, engineStatus, startEngine, Substance, type Env } from "./lib/engine.js";
import { describeGraph, describeRendering, describeSubstance, graphText, inputsText, pixels, renderingText, substanceText } from "./lib/describe.js";
import { cleanName, DEFAULT_SIZE, sizeOf, type SbsarMeta } from "./lib/graph.js";
import { importArchivePaths } from "./lib/import.js";
import { givenParameters } from "./lib/params.js";
import { locateSubstanceArchive, newMaterialId, parentOf, planRender, renderedWith, rerenderMaterial, takenBy, type SubstanceAsset } from "./lib/rerender.js";
import { absolutePath, isSubstance, readSubstanceMeta } from "./lib/substance.js";

const message = (failure: unknown) => (failure instanceof Error ? failure.message : String(failure));
const SETTING_HINT = "tbx plugin settings com.toybox.material-sbsar engine <path to substance_remote_engine>";

/** The `--size` option (pixels) as the engine counts it (a power of two), if given. */
const sizeFromOption = (run: ImportRun | CommandRun): number | undefined => (run.options.size === undefined ? undefined : sizeOf(Math.round(Math.log2(Number(run.options.size)))));

const environment = (run: ImportRun | CommandRun): Env => ({
  services: run.services,
  fs: run.fs,
  renderer: typeof run.options.renderer === "string" && run.options.renderer !== "auto" ? run.options.renderer : undefined,
  log: run.log,
});

/** Run `work` with a clean-up of the render folders this run made, whatever happens. */
async function tidy<T>(env: Env, work: () => Promise<T>): Promise<T> {
  try { return await work(); } finally { await cleanupRenders(env); }
}

export default definePlugin({
  cli: {
    importers: {
      sbsar: (run) => {
        const env = environment(run);
        let substances: string[] = [];
        return tidy(env, async () => {
          const result = await runImport(
          run,
          async (libraryRoot) => {
            const size = sizeFromOption(run) ?? sizeOf((await run.services.settings.get()).size, DEFAULT_SIZE);
            const given = await givenParameters(run.fs, run.options.values, run.options.set);
            const outcome = await importArchivePaths(env, libraryRoot, run.paths, {
              category: String(run.options.category ?? "substance"),
              size,
              preset: typeof run.options.preset === "string" && run.options.preset ? run.options.preset : undefined,
              everyPreset: run.options.allPresets === true,
              graph: typeof run.options.graph === "string" && run.options.graph ? run.options.graph : undefined,
              given,
              ignoreMissing: run.options.ignoreMissing === true,
              skipExisting: run.options.skipExisting === true,
              name: typeof run.options.name === "string" ? run.options.name : undefined,
            }, (id) => run.host.getAssetAsync(id), run.log);
            substances = outcome.substances;
            return outcome;
          },
          (outcome) => (outcome.skippedNotes.length ? `Nothing could be rendered. ${outcome.skippedNotes.join("; ")}` : `No Substance archives (.sbsar) found in ${run.paths.join(", ")}.`),
          );
          // A new substance is also said on a line of its own. (`tbx` lists as imported every id it committed, the
          // substance's too, whatever this handler returns as `imported`.)
          if (substances.length) run.log(`Added ${substances.length === 1 ? "1 substance" : `${substances.length} substances`} (archives new to this library): ${substances.join(", ")}`);
          return result;
        });
      },
    },
    commands: {
      "sbsar inputs": async (run: CommandRun) => {
        const file = String(run.args.file ?? "");
        if (!/\.sbsar$/i.test(file)) throw new Error(`${file} is not a Substance archive (.sbsar).`);
        const env = environment(run);
        return tidy(env, async () => {
          const preset = typeof run.options.preset === "string" && run.options.preset ? run.options.preset : undefined;
          const wanted = typeof run.options.graph === "string" && run.options.graph ? run.options.graph.toLowerCase() : undefined;
          const all = await Substance.graphs(env, file);
          const chosen = wanted === undefined ? all : all.filter((graph) => String(graph.index) === wanted || graph.label.toLowerCase() === wanted);
          if (!chosen.length) throw new Error(`The archive has no graph ${run.options.graph} (it has ${all.map((graph) => `${graph.index}: ${graph.label}`).join(", ")}).`);
          if (preset) for (const graph of chosen) if (!graph.presets.some((item) => item.label === preset)) throw new Error(`${graph.label}: has no preset named "${preset}" (it has: ${graph.presets.map((item) => item.label).join(", ") || "none"})`);
          const graphs = chosen.map((graph) => describeGraph(graph, preset));
          for (const graph of graphs) run.print(graphText(graph));
          return { file, graphs };
        });
      },

      "sbsar render": async (run: CommandRun) => {
        const assets = run.assets ?? [];
        if (!assets.length) throw new Error("No item is selected.");
        const saveAs = typeof run.options.saveAs === "string" && run.options.saveAs.trim() ? run.options.saveAs.trim() : undefined;
        const replace = run.options.replace === true;
        if (replace && !saveAs) throw new Error("--replace goes with --save-as NAME: it lets the new material replace one of that name.");
        if (saveAs && assets.length !== 1) throw new Error(`--save-as makes one new material, but ${assets.length} items are selected. Select one item.`);
        if (!saveAs && assets.some((asset) => isSubstance(asset))) throw new Error("A substance is rendered into a new material: add --save-as NAME (one substance at a time). To render a material made from it again, select the material.");
        const libraryRoot = await run.host.getLibraryRoot();
        if (!libraryRoot) throw new Error("The library has no folder to render into.");
        const host = run.host;
        const dryRun = run.options.dryRun === true;
        const env = environment(run);
        const given = await givenParameters(run.fs, run.options.values, run.options.set);
        const preset = typeof run.options.preset === "string" && run.options.preset ? run.options.preset : undefined;
        const reset = run.options.reset === true;
        if (reset && preset) throw new Error("--reset returns to the archive's defaults; it cannot be combined with --preset.");
        const defaultSize = sizeOf((await run.services.settings.get()).size, DEFAULT_SIZE);
        const result = { dryRun, updated: [] as unknown[], created: [] as unknown[], replaced: [] as unknown[], wouldRender: [] as unknown[], skipped: [] as string[] };
        await tidy(env, async () => {
          let index = 0;
          for (const asset of assets) {
            const label = `${asset.name} (${asset.id})`;
            run.log(`Item ${++index} of ${assets.length}: ${asset.name}`);
            // A material is rendered from its substance's archive; a substance from its own.
            let meta: SbsarMeta | undefined;
            let substanceAsset: SubstanceAsset | undefined;
            if (isSubstance(asset)) {
              substanceAsset = asset;
            } else {
              const found = renderedWith(asset);
              if ("reason" in found) { result.skipped.push(`${label}: ${found.reason}`); continue; }
              meta = found.meta;
              const parent = parentOf(asset);
              if ("reason" in parent) { result.skipped.push(`${label}: ${parent.reason}`); continue; }
              substanceAsset = await run.host.getAssetAsync(parent.id).catch(() => undefined);
            }
            const sbsar = readSubstanceMeta(substanceAsset?.meta)?.sbsar ?? meta?.sbsar ?? "the archive";
            if (substanceAsset && isSubstance(substanceAsset)) {
              try { await host.ensureAssetFiles?.([substanceAsset.id]); }
              catch (failure) { result.skipped.push(`${label}: downloading ${sbsar}: ${message(failure)}`); continue; }
            }
            const archive = locateSubstanceArchive(substanceAsset, libraryRoot);
            if ("reason" in archive) { result.skipped.push(`${label}: ${archive.reason}`); continue; }
            let substance: Substance;
            try { substance = await Substance.open(env, absolutePath(archive), meta?.graph ?? 0); }
            catch (failure) { result.skipped.push(`${label}: loading ${sbsar} in the engine: ${message(failure)}`); continue; }
            let plan;
            try {
              plan = planRender(substance, meta, { preset, reset, given, ignoreMissing: run.options.ignoreMissing === true, size: sizeFromOption(run) }, defaultSize);
            } catch (failure) { result.skipped.push(`${label}: ${message(failure)}`); continue; }
            for (const name of plan.ignored) run.warn(`${asset.name}: has no parameter "${name}"; ignored`);
            // A new material may not take the name of one the substance has (as the viewer refuses it), unless --replace.
            let replaces: AssetView | undefined;
            if (saveAs) {
              const owner = substanceAsset!;
              const siblings = await host.getDerivedAssets(owner.id).catch(() => [] as AssetView[]);
              const byId = await host.getAssetAsync(newMaterialId(owner.id, saveAs)).catch(() => undefined);
              replaces = takenBy(saveAs, siblings, byId);
              if (replaces && !replace) {
                result.skipped.push(`${label}: "${cleanName(saveAs)}" is already a material of ${owner.name} (${replaces.name}, ${replaces.id}); choose another name, or add --replace to replace it`);
                continue;
              }
            }
            const changed = Object.fromEntries(Object.entries(plan.values).filter(([key, value]) => JSON.stringify(value) !== JSON.stringify(substance.graph.inputs.find((input) => input.identifier === key)?.defaultValue)));
            const summary = { id: asset.id, name: asset.name, archive: sbsar, preset: plan.preset, differsFromDefaults: changed, size: plan.size, pixels: pixels(plan.size), mode: saveAs ? `${replaces ? "replace" : "new"} "${saveAs}"` : "update", ...(replaces ? { replaces: replaces.id } : {}) };
            if (dryRun) {
              result.wouldRender.push(summary);
              run.print(`${asset.name}: ${saveAs ? (replaces ? `would save as "${saveAs}", replacing ${replaces.name} (${replaces.id})` : `would save as "${saveAs}"`) : "would update in place"} from ${sbsar}, preset ${plan.preset ?? "none"}, parameters differing from the archive's defaults: ${inputsText(changed as never)}, ${pixels(plan.size)} px`);
              continue;
            }
            try {
              const entry = await rerenderMaterial(env, libraryRoot, { substance: substanceAsset!, ...(meta ? { material: asset } : {}) }, substance,
                { mode: saveAs ? "new" : "update", name: saveAs, ...(replaces ? { replaceId: replaces.id } : {}), preset: plan.preset, values: plan.values, size: plan.size });
              await run.commit([entry]);
              (!saveAs ? result.updated : replaces ? result.replaced : result.created).push({ ...summary, id: saveAs ? entry.id : asset.id });
              if (replaces) run.log(`replaced: ${replaces.name} (${replaces.id})`);
              run.print(saveAs ? entry.id : asset.id);
            } catch (failure) { result.skipped.push(`${label}: ${message(failure)}`); }
          }
        });
        if (!dryRun && !result.updated.length && !result.created.length && !result.replaced.length) run.print("Nothing was rendered.");
        if (dryRun && !result.wouldRender.length) run.print("Nothing would be rendered.");
        const done = result.updated.length + result.created.length + result.replaced.length;
        if (!dryRun) run.log(`${saveAs ? "Saved" : "Updated"} ${done} of ${assets.length} material${assets.length === 1 ? "" : "s"}.`);
        return result;
      },

      "sbsar show": async (run: CommandRun) => {
        const assets: AssetView[] = run.assets ?? [];
        const items: unknown[] = [];
        for (const asset of assets) {
          if (isSubstance(asset)) {
            const item = describeSubstance(asset, await run.host.getDerivedAssets(asset.id).catch(() => []));
            items.push(item);
            run.print(substanceText(item));
            continue;
          }
          const link = asset.kind === "material" ? parentOf(asset) : undefined;
          const parent = link && "id" in link ? await run.host.getAssetAsync(link.id).catch(() => undefined) : undefined;
          const item = describeRendering(asset, parent);
          items.push(item);
          run.print(renderingText(item));
        }
        return { items };
      },

      "sbsar engine": async (run: CommandRun) => {
        const env = environment(run);
        const status = await engineStatus(env);
        const lines = [
          `Substance engine: ${status.program || "(not set)"}${status.exists ? "" : "  -- NOT FOUND"}`,
          `Render with:      ${status.renderer}${status.renderer === "auto" ? " (the graphics card; Direct3D on Windows)" : ""}`,
          `Port ${status.port}:       ${status.running ? "in use (an engine is already running)" : "free"}`,
        ];
        if (!status.exists) {
          for (const line of lines) run.print(line);
          throw new Error(`The Substance engine program was not found${status.program ? ` at ${status.program}` : ""}.\nhint: install Adobe's Substance 3D Integration Tools, then set the program with: ${SETTING_HINT}`);
        }
        let started: { seconds: number; hello: unknown } | undefined;
        if (run.options.start === true) {
          try { started = await startEngine(env); }
          catch (failure) {
            for (const line of lines) run.print(line);
            throw new Error(`${message(failure)}\nhint: if the program is wrong, set it with: ${SETTING_HINT}; if renders fail, try --renderer CPU or VK`);
          }
          lines.push(`Started:          yes, answered after ${started.seconds.toFixed(1)} s${started.hello && Object.keys(started.hello as object).length ? `; it said ${JSON.stringify(started.hello)}` : "; it reports no version"}`);
        } else {
          lines.push("Started:          not tried (add --start to start it and check that it answers)");
        }
        for (const line of lines) run.print(line);
        await cleanupRenders(env);
        return { ...status, started: !!started, startSeconds: started?.seconds, reply: started?.hello ?? null };
      },
    },
  },
});
