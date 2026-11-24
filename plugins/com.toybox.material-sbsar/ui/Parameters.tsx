/**
 * The parameter editor of a Substance graph, used by the substance viewer and
 * the import panel: a search box, the archive's groups as sections that fold,
 * and one control per parameter with the widget the archive's author chose —
 * slider with a number box, switch, list, colour, vector, text, random seed.
 * Each control marks a value that differs from the preset (or the defaults)
 * and can reset it. Rows are memoised and a slider keeps its value to itself
 * while it is dragged, so a graph of a hundred parameters stays responsive.
 */

import { memo, useCallback, useEffect, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { Button, Select, Slider, TextInput } from "@ldlework/toybox-sdk/ui";

import { changedFrom, labelOf, parameterGroups } from "../lib/editor.js";
import { fromHex, rangeOf, toHex, type EngineInput, type Graph, type Value, type Values } from "../lib/graph.js";

/** What the editor shows: the search text and the groups folded away. Kept by the owner, so it outlives a
 *  sidebar tab switch. */
export interface ParameterView {
  query: string;
  closed: string[];
}

export function useParameterView(): [ParameterView, (next: ParameterView) => void] {
  const [view, setView] = useState<ParameterView>({ query: "", closed: [] });
  return [view, setView];
}

interface Props {
  graph: Graph;
  values: Values;
  /** The values of the chosen preset (or the defaults): a value that differs is marked and can be reset to it. */
  base: Values;
  disabled?: boolean;
  onChange(identifier: string, value: Value): void;
  /** Set every parameter back to `base`. */
  onResetAll(): void;
  view: ParameterView;
  onView(next: ParameterView): void;
}

const styles = {
  toolbar: { display: "flex", gap: 8, alignItems: "center" } as CSSProperties,
  groupButton: { display: "flex", alignItems: "center", gap: 6, width: "100%", padding: "6px 0", border: 0, borderBottom: "1px solid var(--border)", background: "transparent", color: "var(--text-1)", font: "inherit", fontSize: 12, fontWeight: 600, textAlign: "left", cursor: "pointer" } as CSSProperties,
  groupCount: { marginLeft: "auto", fontWeight: 400, color: "var(--text-2)" } as CSSProperties,
  rows: { display: "grid", gap: 14, padding: "10px 0 4px" } as CSSProperties,
  inline: { display: "flex", gap: 6, alignItems: "center", minWidth: 0 } as CSSProperties,
  changed: { color: "var(--accent)", fontSize: 11, whiteSpace: "nowrap" } as CSSProperties,
  reset: { border: 0, background: "transparent", color: "var(--text-2)", cursor: "pointer", padding: "0 2px", font: "inherit", fontSize: 12 } as CSSProperties,
  hint: { margin: 0, color: "var(--text-2)" } as CSSProperties,
  /** Read by screen readers, not shown (the row's visible label says the same). */
  hidden: { position: "absolute", width: 1, height: 1, overflow: "hidden", clip: "rect(0 0 0 0)", whiteSpace: "nowrap" } as CSSProperties,
  swatch: { width: 40, height: 28, flex: "none", padding: 0, border: "1px solid var(--border)", borderRadius: 6, background: "none", cursor: "pointer" } as CSSProperties,
};

/** The "changed" mark and the reset button after a label. */
function Changed({ label, changed, disabled, onReset }: { label: string; changed: boolean; disabled?: boolean; onReset(): void }): ReactNode {
  if (!changed) return null;
  return <span style={styles.inline}>
    <span style={styles.changed} title="Differs from the preset">● changed</span>
    <button type="button" style={styles.reset} disabled={disabled} aria-label={`Reset ${label}`} title={`Reset ${label} to the preset's value`} onClick={onReset}>↺</button>
  </span>;
}

/** A labelled row in the slider's layout, for the controls that are not sliders. */
function Row({ label, extra, htmlFor, children }: { label: string; extra: ReactNode; htmlFor?: string; children: ReactNode }): ReactNode {
  return <div className="ds-slider">
    <div className="ds-slider__head">{htmlFor ? <label className="ds-slider__label" htmlFor={htmlFor}>{label}</label> : <span className="ds-slider__label">{label}</span>}{extra}</div>
    {children}
  </div>;
}

/** A number slider that keeps the dragged value to itself and tells the owner once it is let go. */
function NumberControl({ input, value, label, extra, disabled, onCommit }: { input: EngineInput; value: number; label: string; extra: ReactNode; disabled?: boolean; onCommit(value: number): void }) {
  const { min, max, step } = rangeOf(input);
  const [draft, setDraft] = useState<number | null>(null);
  useEffect(() => setDraft(null), [value]);
  const shown = Math.min(max, Math.max(min, draft ?? value));
  return <Slider label={label} value={shown} min={min} max={max} step={step} disabled={disabled} extra={extra}
    onChange={setDraft} onCommit={(next) => { setDraft(null); onCommit(next); }} />;
}

/** A colour: the system picker and the hex text, which is applied when it is a whole `#rrggbb`. */
function ColourControl({ id, value, label, extra, disabled, onChange }: { id: string; value: number[]; label: string; extra: ReactNode; disabled?: boolean; onChange(value: Value): void }) {
  const hex = toHex(value);
  const [text, setText] = useState(hex);
  useEffect(() => setText(hex), [hex]);
  const apply = () => { if (/^#?[0-9a-f]{6}$/i.test(text.trim())) onChange(fromHex(text.trim().startsWith("#") ? text.trim() : `#${text.trim()}`, value)); else setText(hex); };
  return <Row label={label} extra={extra} htmlFor={id}>
    <span style={styles.inline}>
      <input id={id} type="color" value={hex} disabled={disabled} style={styles.swatch} onChange={(event) => onChange(fromHex(event.currentTarget.value, value))} />
      <TextInput aria-label={`${label}, hex`} value={text} disabled={disabled} spellCheck={false} style={{ flex: 1, minWidth: 0, fontFamily: "ui-monospace, monospace" }}
        onChange={(event) => setText(event.currentTarget.value)} onBlur={apply} onKeyDown={(event) => { if (event.key === "Enter") apply(); }} />
    </span>
  </Row>;
}

interface ControlProps {
  input: EngineInput;
  value: Value;
  /** The preset's value; undefined when the archive has none for it. */
  base: Value | undefined;
  changed: boolean;
  disabled?: boolean;
  onChange(identifier: string, value: Value): void;
}

/** One parameter. Memoised: it renders again only when its own value, mark or state changes. */
const Control = memo(function Control({ input, value, base, changed, disabled, onChange }: ControlProps) {
  const id = `sbsar-${input.identifier}`;
  const label = labelOf(input);
  const set = (next: Value) => onChange(input.identifier, next);
  const extra = <Changed label={label} changed={changed} disabled={disabled} onReset={() => { if (base !== undefined) set(base); }} />;
  const scalar = typeof value === "number" ? value : 0;

  let control: ReactNode;
  if (input.identifier === "$randomseed") {
    control = <Row label={label} extra={extra} htmlFor={id}>
      <span style={styles.inline}>
        <TextInput id={id} type="number" min={0} step={1} value={String(scalar)} disabled={disabled} style={{ flex: 1, minWidth: 0 }}
          onChange={(event) => set(Math.max(0, Math.round(Number(event.currentTarget.value) || 0)))} />
        <Button disabled={disabled} onClick={() => set(Math.floor(Math.random() * 100000))}>Shuffle</Button>
      </span>
    </Row>;
  } else if (input.guiWidget === "Togglebutton" || input.type === "boolean") {
    control = <div className="ds-slider__head">
      <label className="ds-check" style={{ flex: 1, minWidth: 0 }}>
        <input type="checkbox" checked={scalar !== 0} disabled={disabled} onChange={(event) => set(event.currentTarget.checked ? 1 : 0)} />
        <span>{label}{input.labelTrue && input.labelFalse && input.labelTrue !== "True" ? ` (${scalar !== 0 ? input.labelTrue : input.labelFalse})` : ""}</span>
      </label>{extra}
    </div>;
  } else if (input.enumValues.length > 0) {
    control = <Row label={label} extra={extra}>
      <label style={{ display: "block" }}><span style={styles.hidden}>{label}</span>
        <Select value={String(scalar)} disabled={disabled} options={input.enumValues.map((item) => ({ value: String(item.value), label: item.label }))} onChange={(next) => set(Number(next))} />
      </label>
    </Row>;
  } else if (input.guiWidget === "Color" && Array.isArray(value) && value.length >= 3) {
    control = <ColourControl id={id} value={value} label={label} extra={extra} disabled={disabled} onChange={set} />;
  } else if (Array.isArray(value)) {
    const whole = input.type.startsWith("integer");
    control = <Row label={label} extra={extra}>
      <span style={styles.inline}>{value.map((part, index) => <TextInput key={index} type="number" step={whole ? 1 : 0.01} value={String(part)} disabled={disabled}
        aria-label={`${label}, ${"XYZW"[index] ?? index + 1}`} style={{ flex: 1, minWidth: 0 }}
        onChange={(event) => { const next = Number(event.currentTarget.value); if (Number.isFinite(next)) set(value.map((old, at) => (at === index ? (whole ? Math.round(next) : next) : old))); }} />)}</span>
    </Row>;
  } else if (typeof value === "string") {
    control = <Row label={label} extra={extra} htmlFor={id}>
      <TextInput id={id} value={value} disabled={disabled} onChange={(event) => set(event.currentTarget.value)} />
    </Row>;
  } else {
    control = <NumberControl input={input} value={scalar} label={label} extra={extra} disabled={disabled} onCommit={set} />;
  }
  return <div title={input.guiDescription || undefined}>{control}</div>;
});

export function Parameters({ graph, values, base, disabled, onChange, onResetAll, view, onView }: Props) {
  const searching = view.query.trim() !== "";
  const groups = useMemo(() => parameterGroups(graph, values, view.query), [graph, values, view.query]);
  const changed = useMemo(() => changedFrom(values, base), [values, base]);
  const total = useMemo(() => parameterGroups(graph, values).reduce((sum, group) => sum + group.inputs.length, 0), [graph, values]);
  const toggle = useCallback((group: string) => {
    onView({ ...view, closed: view.closed.includes(group) ? view.closed.filter((item) => item !== group) : [...view.closed, group] });
  }, [view, onView]);

  if (!total) return <p style={styles.hint}>This archive has no adjustable parameters.</p>;
  return <div style={{ display: "grid", gap: 10 }}>
    <div style={styles.toolbar}>
      <TextInput type="search" aria-label="Search parameters" placeholder={`Search ${total} parameters`} value={view.query} style={{ flex: 1, minWidth: 0 }}
        onChange={(event) => onView({ ...view, query: event.currentTarget.value })} />
      <Button variant="ghost" disabled={disabled || !changed.size} onClick={onResetAll} title="Set every parameter back to the preset's value">Reset all</Button>
    </div>
    {changed.size > 0 && <p style={{ ...styles.hint, fontSize: 12 }}>{changed.size} {changed.size === 1 ? "parameter differs" : "parameters differ"} from the preset.</p>}
    {!groups.length && <p style={styles.hint} role="status">No parameter matches “{view.query.trim()}”.</p>}
    {groups.map(({ group, inputs }) => {
      const open = !group || searching || !view.closed.includes(group);
      const differing = inputs.filter((input) => changed.has(input.identifier)).length;
      const rows = <div style={styles.rows}>{inputs.map((input) => <Control key={input.identifier} input={input} value={values[input.identifier]} base={base[input.identifier]}
        changed={changed.has(input.identifier)} disabled={disabled} onChange={onChange} />)}</div>;
      if (!group) return <div key="">{rows}</div>;
      return <section key={group}>
        <button type="button" style={styles.groupButton} aria-expanded={open} disabled={searching} onClick={() => toggle(group)}>
          <span aria-hidden="true" style={{ width: 10 }}>{open ? "▾" : "▸"}</span>
          <span>{group}</span>
          <span style={styles.groupCount}>{differing ? `${differing} changed · ` : ""}{inputs.length}</span>
        </button>
        {open && rows}
      </section>;
    })}
  </div>;
}
