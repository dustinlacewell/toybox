/**
 * The export-config fallthrough, in one place: a plugin that ships an
 * `ui.exportPanel` module renders its own React component (in an error boundary,
 * with a declarative-fields fallback if it also declared `fields`); otherwise the
 * host renders the declarative `fields`; otherwise nothing. Both paths report
 * their config up through the same `onConfig(values, ready)` contract.
 */

import { useEffect, useMemo } from "react";
import type { FieldSpec, ExportPanelCtx } from "@ldlework/toybox-sdk";
import type { LoadedExporter } from "../../services/pluginRegistry";
import { buildSlotHost } from "../../services/slotHost";
import { PluginSlot } from "./PluginSlot";
import { DeclarativeExportFields } from "./DeclarativeExportFields";

interface Props {
  initialConfig?: Record<string, unknown>;
  plugin: LoadedExporter;
  shared: { targetDir: string | null; preserveStructure: boolean };
  onConfig: (values: Record<string, unknown>, ready: boolean) => void;
}

const EMPTY_FIELDS: FieldSpec[] = [];

export function ExportPanelHost({ plugin, shared, onConfig, initialConfig }: Props) {
  const slot = plugin.manifest.ui?.exportPanel;
  const fields = (plugin.manifest.fields as FieldSpec[]) ?? EMPTY_FIELDS;
  useEffect(() => {
    if (!slot && !fields.length) onConfig(initialConfig ?? {}, true);
  }, [slot, fields, initialConfig, onConfig]);
  const ctx: ExportPanelCtx = useMemo(() => ({ host: buildSlotHost(), shared, initialConfig, setConfig: onConfig }),
    [shared.targetDir, shared.preserveStructure, initialConfig, onConfig]);
  const fallback = fields.length ? (
    <DeclarativeExportFields specs={fields} initialConfig={initialConfig} onConfig={onConfig} />
  ) : null;

  if (!slot) return fallback;

  return (
    <PluginSlot
      pluginId={plugin.manifest.id}
      moduleRel={slot}
      ctx={ctx}
      fallback={fallback}
    />
  );
}
