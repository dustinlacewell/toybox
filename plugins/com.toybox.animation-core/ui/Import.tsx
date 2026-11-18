/**
 * The animation importer panel. Animation clips are pulled out of model files,
 * so the scanning and the file formats come from the Models plugin's
 * `model.api` service, read here with `ctx.extensions`. The panel chooses the
 * files and commits animation entries; the host extracts the clips. An
 * animation import keeps no original files: the source is replaced by its clips.
 */

import { useState } from "react";

import { Stack, Button, Select, Checkbox } from "@ldlework/toybox-sdk/ui";
import { defineImportPanel, importNamespace, type ImportPanelCtx } from "@ldlework/toybox-sdk";
import type { ModelFile } from "com.toybox.model-core";

import { findModelApi, MODEL_API } from "../lib/modelApi.js";
import { importAnimations } from "../lib/run.js";

const CATEGORIES = [
  "animations", "props", "buildings", "characters", "environment", "vehicles",
  "weapons", "fx", "icons", "primitives", "roads", "signs",
] as const;

type Phase =
  | { step: "idle" }
  | { step: "scanned"; source: string; found: ModelFile[] }
  | { step: "importing" }
  | { step: "done"; imported: number; skipped: number; warnings: string[] }
  | { step: "error"; message: string };

export default defineImportPanel(function ImportPanel({ ctx }: { ctx: ImportPanelCtx }) {
  const [pack, setPack] = useState("_import_pending");
  const [category, setCategory] = useState<string>("animations");
  const [phase, setPhase] = useState<Phase>({ step: "idle" });
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const api = findModelApi(ctx.extensions(MODEL_API));
  const looksFor = api?.lookingFor() ?? "";
  const hasFormats = (api?.formats().length ?? 0) > 0;

  const scan = async () => {
    if (!api) return;
    const source = await ctx.host.pickDirectory();
    if (!source) return;
    setPack(importNamespace(source));
    setExcluded(new Set());
    try {
      setPhase({ step: "scanned", source, found: await api.scan(source) });
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  const runImport = async (source: string, files: ModelFile[]) => {
    if (!api) return;
    setPhase({ step: "importing" });
    try {
      const libraryRoot = await ctx.host.getLibraryRoot();
      if (!libraryRoot) throw new Error("no library configured");
      const result = await importAnimations(api, source, libraryRoot, files, pack, category);
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
          <Select value={category} options={CATEGORIES.map((c) => ({ value: c, label: c }))} onChange={setCategory} />
        </Stack>
        <div className="export__hint">Files are stored automatically in the library by asset type.</div>
      </section>

      {!api ? (
        <div className="export__error">The Models plugin is not available, so animations cannot be imported.</div>
      ) : !hasFormats ? (
        <section className="export__section"><div className="export__hint">{looksFor}</div></section>
      ) : phase.step === "importing" ? (
        <div className="export__hint">Importing…</div>
      ) : phase.step === "done" ? (
        <section className="export__section">
          <div className="export__ok">
            Imported {phase.imported} source file{phase.imported === 1 ? "" : "s"}
            {phase.skipped ? `, skipped ${phase.skipped}` : ""}.
          </div>
          {phase.warnings.slice(0, 12).map((w, i) => <div className="export__warn" key={i}>{w}</div>)}
          {phase.warnings.length > 12 && <div className="export__hint">…and {phase.warnings.length - 12} more.</div>}
          <Button onClick={ctx.close}>Done</Button>
        </section>
      ) : phase.step === "scanned" ? (
        <section className="export__section">
          {(() => {
            const chosen = phase.found.filter((f) => !excluded.has(f.relPath));
            return (
              <>
                <div className="export__hint">
                  Found {phase.found.length} source file{phase.found.length === 1 ? "" : "s"}; files without animation clips will be skipped. {looksFor}
                </div>
                <div style={{ maxHeight: "50vh", overflow: "auto" }}>
                  {phase.found.map((f) => (
                    <Checkbox key={f.relPath} label={f.relPath} checked={!excluded.has(f.relPath)} onChange={(included) => {
                      setExcluded((previous) => { const next = new Set(previous); if (included) next.delete(f.relPath); else next.add(f.relPath); return next; });
                    }} />
                  ))}
                </div>
                <Stack dir="row" gap={8}>
                  <Button onClick={() => void scan()}>Choose a different folder…</Button>
                  <Button variant="primary" disabled={!chosen.length || !pack.trim()} onClick={() => void runImport(phase.source, chosen)}>
                    Import {chosen.length} into library
                  </Button>
                </Stack>
              </>
            );
          })()}
        </section>
      ) : (
        <section className="export__section">
          {phase.step === "error" && phase.message && <div className="export__error">{phase.message}</div>}
          <div className="export__hint">{looksFor}</div>
          <Button onClick={() => void scan()}>Choose source folder…</Button>
        </section>
      )}
    </Stack>
  );
});
