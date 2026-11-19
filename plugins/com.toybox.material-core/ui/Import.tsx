/**
 * The material folder importer panel. Walks a user-picked source folder, groups
 * its images into material drafts, lets the user review and fix each one, then
 * copies the used images into private staging and commits one
 * material entry per material.
 *
 * Like the glTF importer, the whole file flow runs here in the panel: every write
 * lands in the library before `ctx.commit`.
 */

import { useState } from "react";

import { Stack, Button, TextInput } from "@ldlework/toybox-sdk/ui";
import {
  defineImportPanel,
  importNamespace,
  type ImportPanelCtx,
} from "@ldlework/toybox-sdk";

import {
  rename,
  setAllIncluded,
  setChannel,
  setIncluded,
  setUse,
  type ReviewItem,
} from "../lib/edit.js";
import { importMaterialItems } from "../lib/import.js";
import { groupMaterials } from "../lib/group.js";
import { ReviewDialog } from "./ReviewDialog.js";
import { useThumbLoader } from "./useThumbLoader.js";
import { findImages } from "../lib/walk.js";
import { materialGroups } from "../lib/variants.js";

type Phase =
  | { step: "idle" }
  | { step: "scanned"; source: string; items: ReviewItem[] }
  | { step: "importing" }
  | { step: "done"; imported: number; skipped: number; warnings: string[] }
  | { step: "error"; message: string };

export default defineImportPanel(function ImportPanel({ ctx }: { ctx: ImportPanelCtx }) {
  const [pack, setPack] = useState("_import_pending");
  const [category, setCategory] = useState("uncategorized");
  const [phase, setPhase] = useState<Phase>({ step: "idle" });
  const loader = useThumbLoader(ctx.fs, phase.step === "scanned" ? phase.source : undefined);

  const scan = async () => {
    const source = await ctx.host.pickDirectory();
    if (!source) return;
    setPack(importNamespace(source));
    try {
      const drafts = groupMaterials(await findImages(ctx.fs, source));
      const items = drafts.map((draft) => ({ draft, included: true }));
      setPhase({ step: "scanned", source, items });
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  const edit = (change: (items: ReviewItem[]) => ReviewItem[]) =>
    setPhase((p) => (p.step === "scanned" ? { ...p, items: change(p.items) } : p));

  const runImport = async (source: string, items: ReviewItem[]) => {
    setPhase({ step: "importing" });
    try {
      const libraryRoot = await ctx.host.getLibraryRoot();
      if (!libraryRoot) throw new Error("no library configured");

      const result = await importMaterialItems({ fs: ctx.fs, sourceRoot: source, libraryRoot }, items, pack, category);
      const grouped = result.entries;
      if (grouped.length) await ctx.commit(grouped);
      setPhase({
        step: "done",
        imported: grouped.length,
        skipped: materialGroups(items).length - grouped.length,
        warnings: result.warnings,
      });
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  if (phase.step === "scanned" && loader) return <ReviewDialog
    key={phase.source} items={phase.items} pack={pack} category={category} source={phase.source}
    loader={loader} Preview={ctx.widgets.MaterialPreview} initialSelected={0} setActions={ctx.setActions}
    onClose={ctx.close} onScan={() => void scan()} onImport={() => void runImport(phase.source, phase.items)}
    onInclude={(i, included) => edit((items) => setIncluded(items, i, included))}
    onIncludeAll={(indexes, included) => edit((items) => setAllIncluded(items, indexes, included))}
    onRename={(i, name) => edit((items) => rename(items, i, name))}
    onUse={(i, f, use) => edit((items) => setUse(items, i, f, use))}
    onChannel={(i, f, c, role) => edit((items) => setChannel(items, i, f, c, role))}
  />;
  return <Stack gap={20}>
    <h2>Bring a texture folder into your library.</h2>
    <p className="export__hint">Review the files, their channels and the material they make before adding them.</p>
    <section className="export__section">
      <div className="export__label">Category</div>
      <Stack dir="row" gap={8}>
        <TextInput aria-label="Category" value={category} onChange={(e) => setCategory(e.currentTarget.value)} placeholder="Category" />
      </Stack>
    </section>
    {phase.step === "scanned" ? <p>Preparing previews…</p> : <PhaseView phase={phase} onScan={() => void scan()} onClose={ctx.close} />}
  </Stack>;
});

/** The phases that need no list: idle, importing, done and error. */
function PhaseView({
  phase,
  onScan,
  onClose,
}: {
  phase: Exclude<Phase, { step: "scanned" }>;
  onScan: () => void;
  onClose: () => void;
}) {
  if (phase.step === "importing") {
    return <div className="export__hint">Importing…</div>;
  }

  if (phase.step === "done") {
    return (
      <section className="export__section">
        <div className="export__ok">
          Imported {phase.imported} material{phase.imported === 1 ? "" : "s"}, skipped{" "}
          {phase.skipped}.
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
