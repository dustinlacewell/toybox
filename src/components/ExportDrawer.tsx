/**
 * Export configuration + run panel. Three intents: a folder of merged .glb, a
 * folder of self-contained loose copies, or a one-way publish into a Godot
 * project's asset_placer library (which additionally writes/merges the addon's
 * asset_library.json). Shows the selected-asset count, the per-intent options,
 * then runs the export and surfaces the report.
 */

import { useState } from "react";

import { Drawer } from "../ds/Drawer";
import { Button } from "../ds/Button";
import { Checkbox } from "../ds/Checkbox";
import { TextInput } from "../ds/TextInput";
import { Stack } from "../ds/Stack";
import { Spinner } from "../ds/Spinner";
import {
  exportCopy,
  exportGlb,
  exportPlacer,
  type PlacerFormat,
  type ExportReport as Report,
} from "../services/tauriApi";
import { pickDirectory } from "../services/pickDirectory";
import { pickSaveFile } from "../services/pickSaveFile";
import "./ExportDrawer.css";

type Mode = "glb" | "copy" | "placer";

interface Props {
  open: boolean;
  selectedIds: string[];
  onClose: () => void;
}

const DEFAULT_SUB_DIR = "assets/exported";

export function ExportDrawer({ open, selectedIds, onClose }: Props) {
  const [mode, setMode] = useState<Mode>("glb");
  const [preserve, setPreserve] = useState(true);
  const [targetDir, setTargetDir] = useState<string | null>(null);
  // Placer-only:
  const [placerFormat, setPlacerFormat] = useState<PlacerFormat>("glb");
  const [subDir, setSubDir] = useState(DEFAULT_SUB_DIR);
  const [libraryJson, setLibraryJson] = useState<string | null>(null);

  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);

  const ready =
    selectedIds.length > 0 &&
    !!targetDir &&
    (mode !== "placer" || !!libraryJson);

  const run = async () => {
    if (!targetDir) return;
    setRunning(true);
    setReport(null);
    setError(null);
    try {
      setReport(await runExport());
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const runExport = (): Promise<Report> => {
    if (mode === "placer") {
      return exportPlacer({
        assetIds: selectedIds,
        targetDir: targetDir!,
        subDir,
        preserveStructure: preserve,
        format: placerFormat,
        libraryJsonPath: libraryJson!,
      });
    }
    const fn = mode === "glb" ? exportGlb : exportCopy;
    return fn(selectedIds, targetDir!, preserve);
  };

  return (
    <Drawer open={open} onClose={onClose}>
      <div className="export">
        <header className="export__header">
          <h2>Export {selectedIds.length} asset{selectedIds.length === 1 ? "" : "s"}</h2>
          <button className="export__close" onClick={onClose}>✕</button>
        </header>

        <section className="export__section">
          <div className="export__label">Destination</div>
          <ModeOption
            active={mode === "glb"}
            onClick={() => setMode("glb")}
            title="Folder of merged .glb (recommended)"
            desc="One self-contained binary file per asset, textures embedded. Lossless."
          />
          <ModeOption
            active={mode === "copy"}
            onClick={() => setMode("copy")}
            title="Folder of self-contained copies"
            desc="Loose .gltf + .bin + textures, paths rewritten. Shared textures deduped."
          />
          <ModeOption
            active={mode === "placer"}
            onClick={() => setMode("placer")}
            title="asset_placer library (Godot)"
            desc="Publish into a Godot project and create/merge its asset_library.json so the dock sees them."
          />
        </section>

        {mode === "placer" && (
          <PlacerOptions
            format={placerFormat}
            onFormat={setPlacerFormat}
            subDir={subDir}
            onSubDir={setSubDir}
            libraryJson={libraryJson}
            onPickLibraryJson={async () =>
              setLibraryJson((await pickSaveFile("asset_library.json")) ?? libraryJson)
            }
          />
        )}

        <section className="export__section">
          <Checkbox
            checked={preserve}
            onChange={setPreserve}
            label="Preserve pack/category folder structure"
          />
        </section>

        <section className="export__section">
          <div className="export__label">
            {mode === "placer" ? "Godot project folder" : "Target folder"}
          </div>
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
          <Button variant="primary" onClick={run} disabled={!ready || running}>
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

/** Options unique to the asset_placer publish: per-file format, target subfolder,
 *  and the asset_library.json to create or merge. */
function PlacerOptions({
  format,
  onFormat,
  subDir,
  onSubDir,
  libraryJson,
  onPickLibraryJson,
}: {
  format: PlacerFormat;
  onFormat: (f: PlacerFormat) => void;
  subDir: string;
  onSubDir: (s: string) => void;
  libraryJson: string | null;
  onPickLibraryJson: () => void;
}) {
  return (
    <>
      <section className="export__section">
        <div className="export__label">Per-asset file format</div>
        <ModeOption
          active={format === "glb"}
          onClick={() => onFormat("glb")}
          title="Merged .glb"
          desc="One embedded binary per asset. Cleanest to drop into a project."
        />
        <ModeOption
          active={format === "copy"}
          onClick={() => onFormat("copy")}
          title="Loose .gltf copy"
          desc="Editable .gltf + .bin + textures in-project."
        />
      </section>

      <section className="export__section">
        <div className="export__label">Project subfolder</div>
        <TextInput
          value={subDir}
          onChange={(e) => onSubDir(e.currentTarget.value)}
          placeholder="assets/exported"
        />
        <div className="export__hint">
          Relative to the project; also the <code>res://</code> prefix for each asset's id.
        </div>
      </section>

      <section className="export__section">
        <div className="export__label">asset_library.json</div>
        <Stack dir="row" gap={8} align="center">
          <Button onClick={onPickLibraryJson}>Choose file…</Button>
          <span className="export__path" title={libraryJson ?? ""}>
            {libraryJson ?? "No file chosen — create or merge"}
          </span>
        </Stack>
      </section>
    </>
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
