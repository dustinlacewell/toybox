/**
 * The model importer panel. Walks a user-picked source folder, hands every file
 * of a format another plugin contributed to `model.formats` to that format,
 * and commits the converted models as seed entries so they appear in the grid.
 * This plugin handles no format itself: with none turned on the panel says so
 * instead of offering a scan.
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
  variantInfo,
  type ImportPanelCtx,
} from "@ldlework/toybox-sdk";

import { MODEL_FORMATS, type ModelFormat } from "../api.js";
import { collectFormats, hasFormats, lookingFor } from "../lib/formats.js";
import { chooseModels, groupModels } from "../lib/choose.js";
import { importModelFiles } from "../lib/import.js";
import { findModels, type Found } from "../lib/walk.js";

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
  const [category, setCategory] = useState<string>("props");
  const [phase, setPhase] = useState<Phase>({ step: "idle" });
  const formats = collectFormats(ctx.extensions<ModelFormat>(MODEL_FORMATS));

  const scan = async () => {
    const source = await ctx.host.pickDirectory();
    if (!source) return;
    setPack(importNamespace(source));
    try {
      const found = await findModels(ctx.fs, source, formats);
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
        formats,
        sourceRoot: source,
        libraryRoot,
      };
      const result = await importModelFiles(io, found, pack, category);
      if (result.entries.length) await ctx.commit(result.entries);
      setPhase({ step: "done", imported: result.entries.length, skipped: result.skipped, warnings: result.warnings });
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
        phase={phase}
        ready={hasFormats(formats)}
        looksFor={lookingFor(formats)}
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
  ready,
  looksFor,
  onScan,
  onImport,
  onClose,
  packReady,
}: {
  phase: Phase;
  /** Whether any format plugin is turned on. */
  ready: boolean;
  /** "Looks for ..." or, when no format is on, the sentence that says so. */
  looksFor: string;
  onScan: () => void;
  onImport: (source: string, found: Found[]) => void;
  onClose: () => void;
  packReady: boolean;
}) {
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [policy, setPolicy] = useState("all");
  if (!ready) {
    return (
      <section className="export__section">
        <div className="export__hint">{looksFor}</div>
      </section>
    );
  }

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
    const groups = groupModels(phase.found);
    const chosen = chooseModels(groups, policy === "best" ? "best" : "all", excluded);
    const count = [...groups.values()].filter((group) => group.some((f) => chosen.includes(f))).length;
    return (
      <section className="export__section">
        <Select value={policy} options={[{ value: "all", label: "All levels" }, { value: "best", label: "Best only" }]} onChange={(v) => { setPolicy(v); setExcluded(new Set()); }} />
        <div className="export__hint">
          Found {groups.size} model{groups.size === 1 ? "" : "s"} under the chosen folder. Animation clips are extracted into separate library assets. {looksFor}
        </div>
        <div style={{ maxHeight: "50vh", overflow: "auto" }}>{[...groups.entries()].map(([name, files]) => <section key={name} style={{ padding: "12px 0", borderBottom: "1px solid var(--border)" }}>
          <strong>{name}</strong>
          {variantInfo(files[0].stem, "model").rank > 0 && <p className="export__warn">Needs a look: full level missing.</p>}
          {files.map((f) => <Checkbox key={f.relPath} label={`${variantInfo(f.stem, "model").label} · ${f.relPath}`} checked={chosen.includes(f)} onChange={(included) => {
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
      <div className="export__hint">{looksFor}</div>
      <Button onClick={onScan}>Choose source folder…</Button>
    </section>
  );
}
