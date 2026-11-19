/**
 * The controls of a Substance graph: one per input a person can change, with
 * the widget the archive's author chose for it — slider, colour, switch or
 * list — grouped as the archive groups them.
 */

import type { CSSProperties, ReactNode } from "react";
import { Button, Checkbox, Select, TextInput } from "@ldlework/toybox-sdk/ui";

import { editableInputs, fromHex, rangeOf, toHex, type EngineInput, type Graph, type Value, type Values } from "../lib/graph.js";

interface Props {
  graph: Graph;
  values: Values;
  disabled?: boolean;
  onChange(identifier: string, value: Value): void;
}

const row: CSSProperties = { display: "grid", gap: 4 };
const labelStyle: CSSProperties = { display: "flex", justifyContent: "space-between", gap: 8, color: "var(--text-1)", fontSize: 12 };
const heading: CSSProperties = { margin: "6px 0 0", color: "var(--text-2)", fontSize: 12, fontWeight: 600, textTransform: "uppercase", letterSpacing: ".04em" };
const format = (value: number, step: number) => (step >= 1 ? String(Math.round(value)) : value.toFixed(step >= 0.1 ? 1 : step >= 0.01 ? 2 : 3));

function Control({ input, value, disabled, onChange }: { input: EngineInput; value: Value; disabled?: boolean; onChange(value: Value): void }): ReactNode {
  const scalar = typeof value === "number" ? value : 0;

  if (input.identifier === "$randomseed") {
    return <label style={row}><span style={labelStyle}>Random seed</span>
      <span style={{ display: "flex", gap: 8 }}>
        <TextInput type="number" min={0} step={1} value={String(scalar)} disabled={disabled} style={{ flex: 1, minWidth: 0 }}
          onChange={(event) => onChange(Math.max(0, Math.round(Number(event.currentTarget.value) || 0)))} />
        <Button disabled={disabled} onClick={() => onChange(Math.floor(Math.random() * 100000))}>Shuffle</Button>
      </span></label>;
  }
  if (input.guiWidget === "Togglebutton" || input.type === "boolean") {
    return <Checkbox checked={scalar !== 0} label={input.label} onChange={(checked) => { if (!disabled) onChange(checked ? 1 : 0); }} />;
  }
  if (input.enumValues.length > 0) {
    return <label style={row}><span style={labelStyle}>{input.label}</span>
      <Select value={String(scalar)} disabled={disabled} options={input.enumValues.map((item) => ({ value: String(item.value), label: item.label }))}
        onChange={(next) => onChange(Number(next))} /></label>;
  }
  if (input.guiWidget === "Color" && Array.isArray(value)) {
    return <label style={row}><span style={labelStyle}><span>{input.label}</span><span>{toHex(value)}</span></span>
      <input type="color" value={toHex(value)} disabled={disabled} aria-label={input.label}
        style={{ width: "100%", height: 28, padding: 0, border: "1px solid var(--border)", borderRadius: 6, background: "none" }}
        onChange={(event) => onChange(fromHex(event.currentTarget.value, value))} /></label>;
  }
  if (Array.isArray(value)) {
    const whole = input.type.startsWith("integer");
    return <div style={row}><span style={labelStyle}>{input.label}</span>
      <span style={{ display: "flex", gap: 6 }}>{value.map((part, index) => <TextInput key={index} type="number" step={whole ? 1 : 0.01} value={String(part)} disabled={disabled}
        aria-label={`${input.label} ${index + 1}`} style={{ flex: 1, minWidth: 0 }}
        onChange={(event) => { const next = Number(event.currentTarget.value); if (Number.isFinite(next)) onChange(value.map((old, at) => (at === index ? (whole ? Math.round(next) : next) : old))); }} />)}</span></div>;
  }
  if (typeof value === "string") {
    return <label style={row}><span style={labelStyle}>{input.label}</span>
      <TextInput value={value} disabled={disabled} onChange={(event) => onChange(event.currentTarget.value)} /></label>;
  }
  const { min, max, step } = rangeOf(input);
  return <label style={row}><span style={labelStyle}><span>{input.label}</span><span>{format(scalar, step)}</span></span>
    <input type="range" min={min} max={max} step={step} value={Math.min(max, Math.max(min, scalar))} disabled={disabled} aria-label={input.label}
      style={{ width: "100%", accentColor: "var(--accent)" }}
      onChange={(event) => onChange(Number(event.currentTarget.value))} /></label>;
}

export function Parameters({ graph, values, disabled, onChange }: Props) {
  const inputs = editableInputs(graph).filter((input) => input.visibleIf && values[input.identifier] !== undefined);
  if (!inputs.length) return <p style={{ margin: 0, color: "var(--text-2)" }}>This archive has no adjustable parameters.</p>;
  // Ungrouped inputs first, then each group in the order it first appears.
  const groups = [...new Set(inputs.map((input) => input.guiGroup))].sort((a, b) => Number(!!a) - Number(!!b));
  return <div style={{ display: "grid", gap: 12 }}>
    {groups.map((group) => <div key={group || "main"} style={{ display: "grid", gap: 10 }}>
      {group && <h4 style={heading}>{group}</h4>}
      {inputs.filter((input) => input.guiGroup === group).map((input) => <div key={input.identifier} title={input.guiDescription || undefined}>
        <Control input={input} value={values[input.identifier]} disabled={disabled} onChange={(value) => onChange(input.identifier, value)} />
      </div>)}
    </div>)}
  </div>;
}
