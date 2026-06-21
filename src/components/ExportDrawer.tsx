/**
 * Export panel. Two built-in destinations — a folder of merged `.glb` or a
 * folder of self-contained loose copies — plus any discovered exporter plugins,
 * rendered side-by-side. Selecting a plugin surfaces its declarative config
 * fields (the Godot asset_placer publish is now one such plugin, not a hardcoded
 * mode). Shows the selected-asset count, the per-mode options, then runs the
 * export and surfaces the report.
 */

import { useCallback, useMemo, useState } from "react";

import { Drawer, Button, Checkbox, Stack, Spinner, Select } from "@ldlework/toybox-sdk/ui";
import {
  exportCopy,
  exportGlb,
  type ExportReport as Report,
} from "../services/tauriApi";
import type { PluginConfig } from "@ldlework/toybox-sdk";
import {
  runExporter,
  type LoadedExporter,
  type PluginLoadError,
} from "../services/pluginRegistry";
import { pickDirectory } from "../services/pickDirectory";
import { ExportPanelHost } from "./plugin-ui/ExportPanelHost";
import "./ExportDrawer.css";

interface Props {
  open: boolean;
  selectedIds: string[];
  exporters: LoadedExporter[];
  pluginErrors: PluginLoadError[];
  onClose: () => void;
}

/** The active destination: a built-in mode or a discovered plugin. */
type Selection =
  | { kind: "native"; id: "glb" | "copy" }
  | { kind: "plugin"; id: string };

const NATIVE_MODES = [
  {
    id: "glb" as const,
    title: "Folder of merged .glb (recommended)",
    desc: "One self-contained binary file per asset, textures embedded. Lossless.",
  },
  {
    id: "copy" as const,
    title: "Folder of self-contained copies",
    desc: "Loose .gltf + .bin + textures, paths rewritten. Shared textures deduped.",
  },
];

export function ExportDrawer({ open, selectedIds, exporters, pluginErrors, onClose }: Props) {
  const [selection, setSelection] = useState<Selection>({ kind: "native", id: "glb" });
  const [preserve, setPreserve] = useState(true);
  const [targetDir, setTargetDir] = useState<string | null>(null);
  // Config + readiness pushed up by whichever export-config path renders (a
  // plugin's custom panel, or the host's declarative fields).
  const [panelConfig, setPanelConfig] = useState<Record<string, unknown>>({});
  const [panelReady, setPanelReady] = useState(false);

  const [running, setRunning] = useState(false);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);

  const activePlugin =
    selection.kind === "plugin"
      ? exporters.find((e) => e.manifest.id === selection.id)
      : undefined;

  // Stable so the panel's config-reporting effects don't re-fire each render.
  const onPanelConfig = useCallback((values: Record<string, unknown>, ready: boolean) => {
    setPanelConfig(values);
    setPanelReady(ready);
  }, []);

  // Native modes + discovered exporters as one dropdown. Each option's value
  // encodes its kind; the description for the selected mode shows beneath.
  const options = useMemo(
    () => [
      ...NATIVE_MODES.map((m) => ({ value: `native:${m.id}`, label: m.title, desc: m.desc })),
      ...exporters.map((e) => ({
        value: `plugin:${e.manifest.id}`,
        label: e.manifest.name,
        desc: e.manifest.description ?? "",
      })),
    ],
    [exporters],
  );
  const selectedValue =
    selection.kind === "native" ? `native:${selection.id}` : `plugin:${selection.id}`;
  const selectedDesc = options.find((o) => o.value === selectedValue)?.desc ?? "";

  const onSelectMode = (value: string) => {
    if (value.startsWith("native:")) {
      setSelection({ kind: "native", id: value.slice(7) as "glb" | "copy" });
    } else {
      setSelection({ kind: "plugin", id: value.slice(7) });
      setPanelReady(false);
    }
  };

  const ready =
    selectedIds.length > 0 &&
    !!targetDir &&
    (selection.kind === "native" || panelReady);

  const run = async () => {
    if (!targetDir) return;
    setRunning(true);
    setReport(null);
    setError(null);
    try {
      setReport(await runSelected(targetDir));
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  const runSelected = (target: string): Promise<Report> => {
    if (selection.kind === "native") {
      const fn = selection.id === "glb" ? exportGlb : exportCopy;
      return fn(selectedIds, target, preserve);
    }
    if (!activePlugin) return Promise.reject(new Error("plugin not loaded"));
    const config: PluginConfig = {
      ...panelConfig,
      targetDir: target,
      preserveStructure: preserve,
    };
    return runExporter(activePlugin, selectedIds, config, new AbortController().signal);
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
          <Select value={selectedValue} options={options} onChange={onSelectMode} />
          {selectedDesc && <div className="export__hint">{selectedDesc}</div>}
          {pluginErrors.map((err) => (
            <div className="export__error" key={err.manifest.id}>
              Plugin “{err.manifest.name || err.manifest.id}” failed to load: {err.error}
            </div>
          ))}
        </section>

        {activePlugin && (
          <ExportPanelHost
            key={activePlugin.manifest.id}
            plugin={activePlugin}
            shared={{ targetDir, preserveStructure: preserve }}
            onConfig={onPanelConfig}
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
            {selection.kind === "plugin" ? "Target folder (project root)" : "Target folder"}
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
