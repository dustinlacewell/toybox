/**
 * The material folder importer panel. Walks a user-picked source folder, groups
 * its images into material drafts, lets the user review and fix each one, then
 * copies the used images into `library/<pack>/materials/<name>/` and commits one
 * material entry per material.
 *
 * Like the glTF importer, the whole file flow runs here in the panel: every write
 * lands in the library before `ctx.commit`.
 */

import { useState } from "react";

import { Stack, Button, TextInput } from "@ldlework/toybox-sdk/ui";
import {
  defineImportPanel,
  type ImportPanelCtx,
  type SeedEntryInput,
} from "@ldlework/toybox-sdk";

import {
  importable,
  rename,
  setAllIncluded,
  setChannel,
  setIncluded,
  setUse,
  type ReviewItem,
} from "./edit.js";
import { toEntry } from "./entry.js";
import { groupMaterials } from "./group.js";
import { ReviewDialog } from "./ReviewDialog.js";
import { useThumbLoader } from "./useThumbLoader.js";
import { copyMaterial, findImages } from "./walk.js";

type Phase =
  | { step: "idle" }
  | { step: "scanned"; source: string; items: ReviewItem[] }
  | { step: "importing" }
  | { step: "done"; imported: number; skipped: number; warnings: string[] }
  | { step: "error"; message: string };

export default defineImportPanel(function ImportPanel({ ctx }: { ctx: ImportPanelCtx }) {
  const [pack, setPack] = useState("imported");
  const [category, setCategory] = useState("uncategorized");
  const [phase, setPhase] = useState<Phase>({ step: "idle" });
  const [reviewOpen, setReviewOpen] = useState(false);
  const loader = useThumbLoader(ctx.fs, phase.step === "scanned" ? phase.source : undefined);

  const scan = async () => {
    const source = await ctx.host.pickDirectory();
    if (!source) return;
    try {
      const drafts = groupMaterials(await findImages(ctx.fs, source));
      const items = drafts.map((draft) => ({ draft, included: true }));
      setPhase({ step: "scanned", source, items });
      setReviewOpen(true);
    } catch (e) {
      setPhase({ step: "error", message: String(e) });
    }
  };

  const edit = (change: (items: ReviewItem[]) => ReviewItem[]) =>
    setPhase((p) => (p.step === "scanned" ? { ...p, items: change(p.items) } : p));

  const runImport = async (source: string, items: ReviewItem[]) => {
    setReviewOpen(false);
    setPhase({ step: "importing" });
    try {
      const libraryRoot = await ctx.host.getLibraryRoot();
      if (!libraryRoot) throw new Error("no library configured");

      const io = { fs: ctx.fs, sourceRoot: source, libraryRoot };
      const entries: SeedEntryInput[] = [];
      const warnings: string[] = [];

      for (const { draft } of importable(items, pack, category)) {
        const res = toEntry(draft, pack, category);
        if (!("entry" in res)) continue;
        try {
          await copyMaterial(io, draft, pack);
          entries.push(res.entry);
        } catch (e) {
          warnings.push(`skipped ${draft.name}: ${String(e)}`);
        }
      }

      if (entries.length) await ctx.commit(entries);
      setPhase({
        step: "done",
        imported: entries.length,
        skipped: items.length - entries.length,
        warnings,
      });
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
          <TextInput
            value={category}
            onChange={(e) => setCategory(e.currentTarget.value)}
            placeholder="category"
            style={{ maxWidth: 180 }}
          />
        </Stack>
      </section>

      <PhaseView
        phase={phase}
        onScan={() => void scan()}
        onReview={() => setReviewOpen(true)}
        onClose={ctx.close}
      />

      {phase.step === "scanned" && reviewOpen && loader && (
        <ReviewDialog
          items={phase.items}
          pack={pack}
          category={category}
          loader={loader}
          onClose={() => setReviewOpen(false)}
          onImport={() => void runImport(phase.source, phase.items)}
          onInclude={(i, included) => edit((items) => setIncluded(items, i, included))}
          onIncludeAll={(indexes, included) =>
            edit((items) => setAllIncluded(items, indexes, included))
          }
          onRename={(i, name) => edit((items) => rename(items, i, name))}
          onUse={(i, f, use) => edit((items) => setUse(items, i, f, use))}
          onChannel={(i, f, c, role) => edit((items) => setChannel(items, i, f, c, role))}
        />
      )}
    </Stack>
  );
});

function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function PhaseView({
  phase,
  onScan,
  onReview,
  onClose,
}: {
  phase: Phase;
  onScan: () => void;
  onReview: () => void;
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

  if (phase.step === "scanned") {
    const n = phase.items.length;
    return (
      <section className="export__section">
        <div className="export__hint">
          {n} material{n === 1 ? "" : "s"} found in {folderName(phase.source)}
        </div>
        <Stack dir="row" gap={8}>
          <Button variant="primary" onClick={onReview}>
            Review…
          </Button>
          <Button onClick={onScan}>Choose a different folder…</Button>
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
