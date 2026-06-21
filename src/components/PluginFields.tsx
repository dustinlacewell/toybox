/**
 * Render a plugin's declarative `FieldSpec[]` as a config form, mapping each
 * field type to a design-system primitive (or the native directory/save-file
 * pickers). The collected values are keyed by `FieldSpec.key` and flow into the
 * plugin run as `ctx.config`. This is what replaced the export drawer's old
 * hardcoded `<PlacerOptions>` block.
 */

import type { FieldSpec } from "@ldlework/toybox-sdk";
import { Button, Checkbox, Stack, TextInput } from "@ldlework/toybox-sdk/ui";
import { pickDirectory } from "../services/pickDirectory";
import { pickSaveFile } from "../services/pickSaveFile";

export type FieldValues = Record<string, unknown>;

interface Props {
  specs: FieldSpec[];
  values: FieldValues;
  onChange: (key: string, value: unknown) => void;
}

/** Seed default values for a field set (run when the selected plugin changes). */
export function defaultFieldValues(specs: FieldSpec[]): FieldValues {
  const out: FieldValues = {};
  for (const spec of specs) {
    if ("default" in spec && spec.default !== undefined) out[spec.key] = spec.default;
    else if (spec.type === "checkbox") out[spec.key] = false;
  }
  return out;
}

/** Whether all `required` fields have a non-empty value (drives the run gate). */
export function fieldsSatisfied(specs: FieldSpec[], values: FieldValues): boolean {
  return specs.every((spec) => {
    if (!("required" in spec) || !spec.required) return true;
    const v = values[spec.key];
    return typeof v === "string" ? v.length > 0 : v != null;
  });
}

export function PluginFields({ specs, values, onChange }: Props) {
  return (
    <>
      {specs.map((spec) => (
        <section className="export__section" key={spec.key}>
          <div className="export__label">{spec.label}</div>
          <Field spec={spec} value={values[spec.key]} onChange={(v) => onChange(spec.key, v)} />
          {"hint" in spec && spec.hint && <div className="export__hint">{spec.hint}</div>}
        </section>
      ))}
    </>
  );
}

function Field({
  spec,
  value,
  onChange,
}: {
  spec: FieldSpec;
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  switch (spec.type) {
    case "text":
      return (
        <TextInput
          value={(value as string) ?? ""}
          placeholder={spec.placeholder}
          onChange={(e) => onChange(e.currentTarget.value)}
        />
      );
    case "checkbox":
      return <Checkbox checked={!!value} onChange={onChange} label={spec.label} />;
    case "select":
      return (
        <>
          {spec.options.map((opt) => (
            <button
              key={opt.value}
              className={`export__mode ${value === opt.value ? "is-active" : ""}`}
              onClick={() => onChange(opt.value)}
            >
              <div className="export__mode-title">{opt.label}</div>
              {opt.desc && <div className="export__mode-desc">{opt.desc}</div>}
            </button>
          ))}
        </>
      );
    case "directory":
      return (
        <PickerRow
          value={value as string | undefined}
          placeholder="No folder selected"
          onPick={async () => onChange((await pickDirectory()) ?? value)}
        />
      );
    case "saveFile":
      return (
        <PickerRow
          value={value as string | undefined}
          placeholder="No file chosen — create or merge"
          onPick={async () => onChange((await pickSaveFile(spec.defaultName ?? "")) ?? value)}
        />
      );
  }
}

function PickerRow({
  value,
  placeholder,
  onPick,
}: {
  value: string | undefined;
  placeholder: string;
  onPick: () => void;
}) {
  return (
    <Stack dir="row" gap={8} align="center">
      <Button onClick={onPick}>Choose…</Button>
      <span className="export__path" title={value ?? ""}>
        {value ?? placeholder}
      </span>
    </Stack>
  );
}
