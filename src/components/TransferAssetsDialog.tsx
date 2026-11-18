import { useMemo, useState } from "react";
import { Button, Checkbox, Dialog, Segmented, Select } from "@ldlework/toybox-sdk/ui";
import type { Asset, LibraryInfo } from "../domain/catalog";
import { libraryLabel } from "../domain/remoteLibrary";
import { allThere, type AlsoIn } from "../domain/duplicates";
import {
  moveBlockers, summaryLine, transferTargets,
  type MergedTransferReport, type OnConflict, type TransferMode,
} from "../domain/transfer";
import "./TransferAssetsDialog.css";

export interface TransferChoice {
  toLibrary: string;
  onConflict: OnConflict;
  includeClips: boolean;
}

const plural = (count: number) => `${count} ${count === 1 ? "item" : "items"}`;

/** Copy or move the selection into another library. It does not call the backend
 *  itself: `onRun` does, and reports progress through the callback it is given. */
export function TransferAssetsDialog({ mode, assets, libraries, alsoIn = new Map(), onRun, onClose }: {
  mode: TransferMode;
  assets: Asset[];
  libraries: LibraryInfo[];
  /** Where else each item exists, so a library that already has all of them is not offered. */
  alsoIn?: ReadonlyMap<string, AlsoIn[]>;
  onRun: (choice: TransferChoice, onProgress: (progress: { done: number; total: number }) => void) => Promise<MergedTransferReport>;
  onClose: () => void;
}) {
  const sources = useMemo(() => [...new Set(assets.map((asset) => asset.library))], [assets]);
  const { targets, blocked } = useMemo(() => {
    const all = transferTargets(libraries, sources);
    const there = all.targets.filter((library) => allThere(assets, alsoIn, library.id));
    return {
      targets: all.targets.filter((library) => !there.includes(library)),
      blocked: [...all.blocked, ...there.map((library) => ({ library, reason: "already there" }))],
    };
  }, [libraries, sources, assets, alsoIn]);
  const [target, setTarget] = useState(targets[0]?.id ?? "");
  const [onConflict, setOnConflict] = useState<OnConflict>("skip");
  const [includeClips, setIncludeClips] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number }>();
  const [report, setReport] = useState<MergedTransferReport>();
  const [error, setError] = useState<string>();
  const running = !!progress && !report;
  const verb = mode === "copy" ? "Copy" : "Move";
  const names = (ids: string[]) => ids.map((id) => libraries.find((library) => library.id === id)?.name ?? id).join(", ");
  const blockers = mode === "move" ? moveBlockers(libraries, sources, target) : [];
  const hasModels = assets.some((asset) => asset.kind === "model");
  const why = (list: { library: LibraryInfo; reason: string }[]) => list.map(({ library, reason }) => `${library.name} (${reason})`).join(", ");

  const run = async () => {
    setError(undefined);
    setProgress({ done: 0, total: assets.length });
    try { setReport(await onRun({ toLibrary: target, onConflict, includeClips }, setProgress)); }
    catch (failure) { setError(String(failure).replace(/^Error: /, "")); setProgress(undefined); }
  };

  const title = report ? `${verb} finished` : `${verb} ${plural(assets.length)} to another library`;
  const footer = report
    ? <Button variant="primary" autoFocus onClick={onClose}>Close</Button>
    : <>
      <Button autoFocus disabled={running} onClick={onClose}>Cancel</Button>
      <Button variant="primary" disabled={running || !target || blockers.length > 0 || !assets.length} onClick={() => void run()}>
        {running ? (mode === "copy" ? "Copying…" : "Moving…") : `${verb} ${plural(assets.length)}`}
      </Button>
    </>;

  return <Dialog open size="compact" title={title} onClose={() => { if (!running) onClose(); }} footer={footer}>
    <div className="ds-confirmation" aria-busy={running}>
      {report ? <Report report={report} mode={mode} targetName={names([target])} /> : <>
        <p className="transfer-dialog__summary">{target ? summaryLine(assets, libraries, target) : plural(assets.length)}</p>
        {targets.length === 0
          ? <p className="ds-confirmation__error" role="alert">No other library can be written to{blocked.length ? `: ${why(blocked)}` : ""}.</p>
          : <>
            <div className="transfer-dialog__field">
              <span>Into library</span>
              <Select value={target} options={targets.map((library) => ({ value: library.id, label: libraryLabel(library) }))} onChange={setTarget} disabled={running} />
              {blocked.length > 0 && <small className="ds-confirmation__note">Not offered: {why(blocked)}.</small>}
            </div>
            <div className="transfer-dialog__field">
              <span>If an item already exists there</span>
              <Segmented label="If an item already exists there" value={onConflict} onChange={setOnConflict}
                options={[{ value: "skip", label: "Skip it" }, { value: "replace", label: "Replace it" }]} />
              <small className="ds-confirmation__note">{onConflict === "skip" ? "Items with the same id are left as they are." : "Items with the same id are overwritten; their favourites and tags stay."}</small>
            </div>
            {hasModels && <Checkbox checked={includeClips} onChange={setIncludeClips} label="Also take the animations extracted from these models" />}
          </>}
        {mode === "move" && targets.length > 0 && !blockers.length && <p className="ds-confirmation__note">They will be removed from {names(sources.filter((id) => id !== target))} after they are safely copied.</p>}
        {blockers.length > 0 && <p className="ds-confirmation__error" role="alert">Cannot move out of {why(blockers)}. Copy instead.</p>}
        {progress && <div className="transfer-dialog__field" role="status">
          <progress className="transfer-dialog__progress" value={progress.done} max={Math.max(progress.total, 1)} />
          <small className="ds-confirmation__note">{mode === "copy" ? "Copying" : "Moving"} {Math.min(progress.done, progress.total)} of {progress.total}…</small>
        </div>}
        {error && <p className="ds-confirmation__error" role="alert">{error}</p>}
      </>}
    </div>
  </Dialog>;
}

function Report({ report, mode, targetName }: { report: MergedTransferReport; mode: TransferMode; targetName: string }) {
  const lines = report.items.filter((item) => item.outcome === "skipped" || item.outcome === "failed" || item.reason);
  const done = report.copied + report.replaced;
  return <>
    <div className="transfer-dialog__counts" role="status">
      <span><strong>{report.copied}</strong> copied</span>
      {report.replaced > 0 && <span><strong>{report.replaced}</strong> replaced</span>}
      <span><strong>{report.skipped}</strong> skipped</span>
      <span><strong>{report.failed}</strong> failed</span>
    </div>
    <p className="ds-confirmation__note">
      {done > 0 ? `${plural(done)} now in ${targetName}.` : `Nothing was added to ${targetName}.`}
      {mode === "move" && ` ${report.removed} removed from the source${report.removed === 1 ? "" : "s"}.`}
    </p>
    {lines.length > 0 && <div className="ds-confirmation__items" tabIndex={0} aria-label="Details">
      <ul className="transfer-dialog__lines">
        {lines.map((item) => <li key={item.assetId} className={`transfer-dialog__line transfer-dialog__line--${item.outcome}`}>
          <span title={item.name}>{item.name}</span>
          <small>{item.outcome === "failed" ? "Failed" : item.outcome === "skipped" ? "Skipped" : "Note"}{item.reason ? `: ${item.reason}` : ""}</small>
        </li>)}
      </ul>
    </div>}
  </>;
}
