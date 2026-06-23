/**
 * The glTF/glb importer panel. Walks a user-picked source folder, copies/unpacks
 * every `.gltf`/`.glb` into `library/<pack>/<category>/`, then commits seed
 * entries so the assets appear in the grid.
 *
 * The whole file flow runs here in the panel (not a separate `run` phase) because
 * importers do their work during the panel session: all writes must land in the
 * library before `ctx.commit`, which re-reads each glTF to index it.
 */

import { useState } from "react";

import { Stack, Button, TextInput, Select } from "@ldlework/toybox-sdk/ui";
import {
  defineImportPanel,
  type ImportPanelCtx,
  type SeedEntryInput,
} from "@ldlework/toybox-sdk";

import { findModels, materialize, type Found } from "./walk.js";

const CATEGORIES = [
  "props", "buildings", "characters", "environment", "vehicles",
  "weapons", "fx", "icons", "primitives", "roads", "signs",
] as const;

type Phase =
  | { step: "idle" }
  | { step: "scanned"; source: string; found: Found[] }
  | { step: "importing" }
  | { step: "done"; imported: number; skipped: number; warnings: string[] }
  | { step: "error"; message: string };

export default defineImportPanel(function ImportPanel({ ctx }: { ctx: ImportPanelCtx }) {
  const [pack, setPack] = useState("imported");
  const [category, setCategory] = useState<string>("props");
  const [phase, setPhase] = useState<Phase>({ step: "idle" });

  const scan = async () => {
    const source = await ctx.host.pickDirectory();
    if (!source) return;
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
        entries.push({
          id: `uid://import_${pack}_${category}_${f.stem}`,
          pack,
          category,
          file: result.file,
        });
      }

      if (entries.length) await ctx.commit(entries);
      setPhase({ step: "done", imported: entries.length, skipped, warnings });
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  return (
    <Stack gap={12}>
      <section className="export__section">
        <div className="export__label">Destination</div>
        <Stack dir="row" gap={8}>
          <TextInput
            value={pack}
            onChange={(e) => setPack(e.currentTarget.value)}
            placeholder="pack"
            style={{ maxWidth: 180 }}
          />
          <Select
            value={category}
            options={CATEGORIES.map((c) => ({ value: c, label: c }))}
            onChange={setCategory}
          />
        </Stack>
        <div className="export__hint">
          Every imported asset is filed under library/{pack || "…"}/{category}/.
        </div>
      </section>

      <PhaseView
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
  phase,
  onScan,
  onImport,
  onClose,
  packReady,
}: {
  phase: Phase;
  onScan: () => void;
  onImport: (source: string, found: Found[]) => void;
  onClose: () => void;
  packReady: boolean;
}) {
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
    const count = phase.found.length;
    return (
      <section className="export__section">
        <div className="export__hint">
          Found {count} model{count === 1 ? "" : "s"} under the chosen folder.
        </div>
        <Stack dir="row" gap={8}>
          <Button onClick={onScan}>Choose a different folder…</Button>
          <Button
            variant="primary"
            disabled={!count || !packReady}
            onClick={() => onImport(phase.source, phase.found)}
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
