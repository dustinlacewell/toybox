/**
 * The Substance plugin on the command line:
 *
 * - `tbx import sbsar` renders archives into materials;
 * - `tbx sbsar inputs|render|show|engine` look inside archives, render stored
 *   materials again, show how they were made and check the engine.
 *
 * No user interface; the work is in `lib/`, shared with the import panel and the
 * asset panel.
 */

import { definePlugin, runImport, type AssetView, type CommandRun, type ImportRun } from "@ldlework/toybox-sdk";

import { absolutePath, locateArchive } from "./lib/archive.js";
import { cleanupRenders, engineStatus, startEngine, Substance, type Env } from "./lib/engine.js";
import { describeGraph, describeRendering, graphText, inputsText, pixels, renderingText } from "./lib/describe.js";
import { sizeOf } from "./lib/graph.js";
import { importArchivePaths } from "./lib/import.js";
import { givenParameters } from "./lib/params.js";
import { planRender, renderedWith, rerenderMaterial } from "./lib/rerender.js";

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
        return tidy(env, () => runImport(
          run,
          async (libraryRoot) => {
            const size = sizeFromOption(run) ?? sizeOf((await run.services.settings.get()).size);
            const given = await givenParameters(run.fs, run.options.values, run.options.set);
            return importArchivePaths(env, libraryRoot, run.paths, {
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
          },
          (outcome) => (outcome.skippedNotes.length ? `Nothing could be rendered. ${outcome.skippedNotes.join("; ")}` : `No Substance archives (.sbsar) found in ${run.paths.join(", ")}.`),
        ));
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
        if (saveAs && assets.length !== 1) throw new Error(`--save-as makes one new material, but ${assets.length} items are selected. Select one item.`);
        const libraryRoot = await run.host.getLibraryRoot();
        if (!libraryRoot) throw new Error("The library has no folder to render into.");
        const dryRun = run.options.dryRun === true;
        const env = environment(run);
        const given = await givenParameters(run.fs, run.options.values, run.options.set);
        const preset = typeof run.options.preset === "string" && run.options.preset ? run.options.preset : undefined;
        const reset = run.options.reset === true;
        if (reset && preset) throw new Error("--reset returns to the archive's defaults; it cannot be combined with --preset.");
        const result = { dryRun, updated: [] as unknown[], created: [] as unknown[], wouldRender: [] as unknown[], skipped: [] as string[] };
        await tidy(env, async () => {
          let index = 0;
          for (const asset of assets) {
            const label = `${asset.name} (${asset.id})`;
            run.log(`Material ${++index} of ${assets.length}: ${asset.name}`);
            const found = renderedWith(asset);
            if ("reason" in found) { result.skipped.push(`${label}: ${found.reason}`); continue; }
            const { meta } = found;
            const archive = locateArchive(asset, meta, libraryRoot);
            if (!archive) { result.skipped.push(`${label}: its Substance archive (${meta.sbsar}) is not stored with it`); continue; }
            let substance: Substance;
            try { substance = await Substance.open(env, absolutePath(archive), meta.graph); }
            catch (failure) { result.skipped.push(`${label}: loading ${meta.sbsar} in the engine: ${message(failure)}`); continue; }
            let plan;
            try {
              plan = planRender(substance, meta, { preset, reset, given, ignoreMissing: run.options.ignoreMissing === true, size: sizeFromOption(run) });
            } catch (failure) { result.skipped.push(`${label}: ${message(failure)}`); continue; }
            for (const name of plan.ignored) run.warn(`${asset.name}: has no parameter "${name}"; ignored`);
            const changed = Object.fromEntries(Object.entries(plan.values).filter(([key, value]) => JSON.stringify(value) !== JSON.stringify(substance.graph.inputs.find((input) => input.identifier === key)?.defaultValue)));
            const summary = { id: asset.id, name: asset.name, archive: meta.sbsar, preset: plan.preset, differsFromDefaults: changed, size: plan.size, pixels: pixels(plan.size), mode: saveAs ? `new "${saveAs}"` : "update" };
            if (dryRun) {
              result.wouldRender.push(summary);
              run.print(`${asset.name}: ${saveAs ? `would save as "${saveAs}"` : "would update in place"} from ${meta.sbsar}, preset ${plan.preset ?? "none"}, parameters differing from the archive's defaults: ${inputsText(changed as never)}, ${pixels(plan.size)} px`);
              continue;
            }
            try {
              const entry = await rerenderMaterial(env, libraryRoot, asset as AssetView, meta, substance, { mode: saveAs ? "new" : "update", name: saveAs, preset: plan.preset, values: plan.values, size: plan.size });
              await run.commit([entry]);
              (saveAs ? result.created : result.updated).push({ ...summary, id: saveAs ? entry.id : asset.id });
              run.print(saveAs ? entry.id : asset.id);
            } catch (failure) { result.skipped.push(`${label}: ${message(failure)}`); }
          }
        });
        if (!dryRun && !result.updated.length && !result.created.length) run.print("Nothing was rendered.");
        if (dryRun && !result.wouldRender.length) run.print("Nothing would be rendered.");
        const done = result.updated.length + result.created.length;
        if (!dryRun) run.log(`${saveAs ? "Saved" : "Updated"} ${done} of ${assets.length} material${assets.length === 1 ? "" : "s"}.`);
        return result;
      },

      "sbsar show": async (run: CommandRun) => {
        const assets: AssetView[] = run.assets ?? [];
        const items = assets.map((asset) => describeRendering(asset));
        for (const item of items) run.print(renderingText(item));
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
