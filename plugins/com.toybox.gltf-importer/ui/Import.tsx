/**
 * The glTF/glb importer panel. Walks a user-picked source folder, copies/unpacks
 * every `.gltf`/`.glb` into private staging, then commits seed
 * entries so the assets appear in the grid.
 *
 * The whole file flow runs here in the panel (not a separate `run` phase) because
 * importers do their work during the panel session: all writes must land in the
 * library before `ctx.commit`, which re-reads each glTF to index it.
 */

import { useState } from "react";

import { Stack, Button, Select, Checkbox } from "@ldlework/toybox-sdk/ui";
import {
  defineImportPanel,
  importNamespace,
  groupSeedVariants,
  variantInfo,
  type ImportPanelCtx,
  type SeedEntryInput,
} from "@ldlework/toybox-sdk";

import { findModels, materialize, type Found } from "./walk.js";

const CATEGORIES = [
  "props", "buildings", "characters", "environment", "vehicles",
  "weapons", "fx", "icons", "primitives", "roads", "signs", "animations",
] as const;

type Phase =
  | { step: "idle" }
  | { step: "scanned"; source: string; found: Found[] }
  | { step: "importing" }
  | { step: "done"; imported: number; skipped: number; warnings: string[] }
  | { step: "error"; message: string };

export default defineImportPanel(function ImportPanel({ ctx }: { ctx: ImportPanelCtx }) {
  const [pack, setPack] = useState("_import_pending");
  const [category, setCategory] = useState<string>(ctx.kind === "animation" ? "animations" : "props");
  const [phase, setPhase] = useState<Phase>({ step: "idle" });

  const scan = async () => {
    const source = await ctx.host.pickDirectory();
    if (!source) return;
    setPack(importNamespace(source));
    try {
      const found = await findModels(ctx.fs, source);
      setPhase({ step: "scanned", source, found });
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  const runImport = async (source: string, found: Found[]) => {
    setPhase({ step: "importing" });
    try {
      const libraryRoot = await ctx.host.getLibraryRoot();
      if (!libraryRoot) throw new Error("no library configured");

      const io = {
        fs: ctx.fs,
        convert: ctx.host.convertToGltf,
        sourceRoot: source,
        libraryRoot,
      };
      const entries: SeedEntryInput[] = [];
      const warnings: string[] = [];
      const seenStems = new Set<string>();
      let skipped = 0;

      for (const f of found) {
        // Two sources with the same basename collide on both the on-disk
        // `<stem>.gltf` and the catalog id, so the second would silently
        // overwrite/duplicate. Keep the first, skip the rest.
        if (seenStems.has(f.stem)) {
          skipped += 1;
          warnings.push(`skipped duplicate name: ${f.relPath}`);
          continue;
        }

        const result = await materialize(io, f, pack, category);
        if ("rejected" in result) {
          skipped += 1;
          warnings.push(`skipped ${f.relPath}: ${result.rejected}`);
          continue;
        }
        seenStems.add(f.stem);
        warnings.push(...result.warnings);
        if (ctx.kind === "animation") {
          const json = JSON.parse(new TextDecoder().decode(await ctx.fs.readBytes(libraryRoot, `library/${pack}/${category}/${result.file}`)));
          if (!Array.isArray(json.animations) || !json.animations.some((clip: { channels?: unknown[] }) => clip.channels?.length)) {
            skipped += 1; warnings.push(`No animation clips in ${f.relPath}`); continue;
          }
          entries.push({ kind: "animation", id: `uid://import_${pack}_${category}_animations_${f.stem}`, pack, category, file: result.file });
          continue;
        }
        entries.push({
          id: `uid://import_${pack}_${category}_${f.stem}`,
          pack,
          category,
          file: result.file,
        });
      }

      const grouped = groupSeedVariants(entries);
      if (grouped.length) await ctx.commit(grouped);
      setPhase({ step: "done", imported: grouped.length, skipped, warnings });
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  return (
    <Stack gap={12}>
      <section className="export__section">
        <div className="export__label">Category</div>
        <Stack dir="row" gap={8}>
          <Select
            value={category}
            options={CATEGORIES.map((c) => ({ value: c, label: c }))}
            onChange={setCategory}
          />
        </Stack>
        <div className="export__hint">
          Files are stored automatically in the library by asset type.
        </div>
      </section>

      <PhaseView
        kind={ctx.kind === "animation" ? "animation" : "model"}
        phase={phase}
        onScan={() => void scan()}
        onImport={(s, f) => void runImport(s, f)}
        onClose={ctx.close}
        packReady={pack.trim().length > 0}
      />
    </Stack>
  );
});

function PhaseView({
  kind,
  phase,
  onScan,
  onImport,
  onClose,
  packReady,
}: {
  kind: "model" | "animation";
  phase: Phase;
  onScan: () => void;
  onImport: (source: string, found: Found[]) => void;
  onClose: () => void;
  packReady: boolean;
}) {
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [policy, setPolicy] = useState("all");
  if (phase.step === "importing") {
    return <div className="export__hint">Importing…</div>;
  }

  if (phase.step === "done") {
    return (
      <section className="export__section">
        <div className="export__ok">
          Imported {phase.imported} asset{phase.imported === 1 ? "" : "s"}
          {phase.skipped ? `, skipped ${phase.skipped}` : ""}.
        </div>
        {phase.warnings.slice(0, 12).map((w, i) => (
          <div className="export__warn" key={i}>{w}</div>
        ))}
        {phase.warnings.length > 12 && (
          <div className="export__hint">…and {phase.warnings.length - 12} more.</div>
        )}
        <Button onClick={onClose}>Done</Button>
      </section>
    );
  }

  if (phase.step === "scanned") {
    const groups = new Map<string, Found[]>();
    for (const file of phase.found) {
      const info = variantInfo(file.stem, kind);
      const group = groups.get(info.base) ?? [];
      group.push(file); groups.set(info.base, group);
    }
    for (const group of groups.values()) group.sort((a, b) => variantInfo(a.stem, kind).rank - variantInfo(b.stem, kind).rank);
    const chosen = [...groups.values()].flatMap((group) => (policy === "best" ? group.slice(0, 1) : group).filter((f) => !excluded.has(f.relPath)));
    const count = [...groups.values()].filter((group) => group.some((f) => chosen.includes(f))).length;
    return (
      <section className="export__section">
        {kind === "model" && <Select value={policy} options={[{ value: "all", label: "All levels" }, { value: "best", label: "Best only" }]} onChange={(v) => { setPolicy(v); setExcluded(new Set()); }} />}
        <div className="export__hint">
          Found {groups.size} {kind === "animation" ? "source files; files without animation clips will be skipped" : `model${groups.size === 1 ? "" : "s"}`} under the chosen folder.
          {kind === "model" && " Animation clips are extracted into separate library assets."}
        </div>
        <div style={{ maxHeight: "50vh", overflow: "auto" }}>{[...groups.entries()].map(([name, files]) => <section key={name} style={{ padding: "12px 0", borderBottom: "1px solid var(--border)" }}>
          <strong>{name}</strong>
          {kind === "model" && variantInfo(files[0].stem, "model").rank > 0 && <p className="export__warn">Needs a look: full level missing.</p>}
          {files.map((f) => <Checkbox key={f.relPath} label={kind === "animation" ? f.relPath : `${variantInfo(f.stem, "model").label} · ${f.relPath}`} checked={chosen.includes(f)} onChange={(included) => {
            setExcluded(() => { const next = new Set(phase.found.filter((file) => !chosen.includes(file)).map((file) => file.relPath)); if (included) next.delete(f.relPath); else next.add(f.relPath); return next; });
            setPolicy("all");
          }} />)}
        </section>)}</div>
        <Stack dir="row" gap={8}>
          <Button onClick={() => { setExcluded(new Set()); onScan(); }}>Choose a different folder…</Button>
          <Button
            variant="primary"
            disabled={!count || !packReady}
            onClick={() => onImport(phase.source, chosen)}
          >
            Import {count} into library
          </Button>
        </Stack>
      </section>
    );
  }

  // idle or error
  return (
    <section className="export__section">
      {phase.step === "error" && phase.message && (
        <div className="export__error">{phase.message}</div>
      )}
      <Button onClick={onScan}>Choose source folder…</Button>
    </section>
  );
}
