/**
 * Adapter that renders a plugin's declarative `fields` (the zero-code path) and
 * pushes the collected values up via the same `setConfig(values, ready)` contract
 * a custom export panel uses. Lets `ExportDrawer` treat both paths uniformly —
 * the unchanged `<PluginFields>` underneath still maps each `FieldSpec` to a ds
 * primitive.
 */

import { useEffect, useState } from "react";

import type { FieldSpec } from "@ldlework/toybox-sdk";
import {
  PluginFields,
  defaultFieldValues,
  fieldsSatisfied,
  type FieldValues,
} from "../PluginFields";

interface Props {
  specs: FieldSpec[];
  onConfig: (values: Record<string, unknown>, ready: boolean) => void;
}

export function DeclarativeExportFields({ specs, onConfig }: Props) {
  const [values, setValues] = useState<FieldValues>(() => defaultFieldValues(specs));

  // Re-seed defaults when the field set changes (a different plugin selected).
  useEffect(() => setValues(defaultFieldValues(specs)), [specs]);

  // Push current values + readiness up whenever they change.
  useEffect(() => onConfig(values, fieldsSatisfied(specs, values)), [onConfig, specs, values]);

  const change = (key: string, value: unknown) =>
    setValues((prev) => ({ ...prev, [key]: value }));

  return <PluginFields specs={specs} values={values} onChange={change} />;
}
