/**
 * Export configuration + run panel. Shows the selected-asset count, lets the
 * user choose the mode (merged .glb default, or self-contained loose copy), the
 * layout (preserve pack/category structure vs. flatten), and a target folder,
 * then runs the export and surfaces the report.
 */

import { useState } from "react";

import { Drawer } from "../ds/Drawer";
import { Button } from "../ds/Button";
import { Checkbox } from "../ds/Checkbox";
import { Stack } from "../ds/Stack";
import { Spinner } from "../ds/Spinner";
import {
  exportCopy,
  exportGlb,
  type ExportReport as Report,
} from "../services/tauriApi";
import { pickDirectory } from "../services/pickDirectory";
import "./ExportDrawer.css";

type Mode = "glb" | "copy";

interface Props {
  open: boolean;
  selectedIds: string[];
  onClose: () => void;
}

export function ExportDrawer({ open, selectedIds, onClose }: Props) {
  const [mode, setMode] = useState<Mode>("glb");
  const [preserve, setPreserve] = useState(true);
  const [targetDir, setTargetDir] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    if (!targetDir) return;
    setRunning(true);
    setReport(null);
    setError(null);
    try {
      const fn = mode === "glb" ? exportGlb : exportCopy;
      setReport(await fn(selectedIds, targetDir, preserve));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Drawer open={open} onClose={onClose}>
      <div className="export">
        <header className="export__header">
          <h2>Export {selectedIds.length} asset{selectedIds.length === 1 ? "" : "s"}</h2>
          <button className="export__close" onClick={onClose}>✕</button>
        </header>

        <section className="export__section">
          <div className="export__label">Format</div>
          <ModeOption
            active={mode === "glb"}
            onClick={() => setMode("glb")}
            title="Merged .glb (recommended)"
            desc="One self-contained binary file per asset, textures embedded. Lossless."
          />
          <ModeOption
            active={mode === "copy"}
            onClick={() => setMode("copy")}
            title="Self-contained copy"
            desc="Loose .gltf + .bin + textures, paths rewritten. Shared textures deduped."
          />
        </section>

        <section className="export__section">
          <Checkbox
            checked={preserve}
            onChange={setPreserve}
            label="Preserve pack/category folder structure"
          />
        </section>

        <section className="export__section">
          <div className="export__label">Destination</div>
          <Stack dir="row" gap={8} align="center">
            <Button onClick={async () => setTargetDir(await pickDirectory())}>
              Choose folder…
            </Button>
            <span className="export__path" title={targetDir ?? ""}>
              {targetDir ?? "No folder selected"}
            </span>
          </Stack>
        </section>

        <section className="export__section">
          <Button
            variant="primary"
            onClick={run}
            disabled={!targetDir || running || selectedIds.length === 0}
          >
            {running ? "Exporting…" : "Export"}
          </Button>
          {running && <Spinner />}
        </section>

        {error && <div className="export__error">{error}</div>}
        {report && <ReportView report={report} />}
      </div>
    </Drawer>
  );
}

function ModeOption({
  active,
  onClick,
  title,
  desc,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  desc: string;
}) {
  return (
    <button className={`export__mode ${active ? "is-active" : ""}`} onClick={onClick}>
      <div className="export__mode-title">{title}</div>
      <div className="export__mode-desc">{desc}</div>
    </button>
  );
}

function ReportView({ report }: { report: Report }) {
  return (
    <section className="export__section export__report">
      <div className="export__ok">✓ Wrote {report.written.length} files</div>
      {report.warnings.length > 0 && (
        <ReportList title="Warnings" items={report.warnings} className="export__warn" />
      )}
      {report.skipped.length > 0 && (
        <ReportList title="Skipped" items={report.skipped} className="export__skip" />
      )}
    </section>
  );
}

function ReportList({
  title,
  items,
  className,
}: {
  title: string;
  items: string[];
  className: string;
}) {
  return (
    <div className={className}>
      <div className="export__label">{title}</div>
      <ul>
        {items.map((it, i) => (
          <li key={i}>{it}</li>
        ))}
      </ul>
    </div>
  );
}
