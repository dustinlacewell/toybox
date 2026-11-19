/**
 * Parameter values given as text (`--set name=value`) or as JSON (`--values file`),
 * checked against an archive's graph and turned into the values the engine takes.
 * Pure: shared by `tbx import sbsar`, `tbx sbsar render` and anything else that
 * sets parameters without a window.
 *
 * Accepted forms, by the input's type:
 *
 * - a number (`float`, `integer`): `0.4`, `12`; an integer must be whole; a slider's
 *   range is enforced (the random seed is not a slider and has none);
 * - a switch: `true`/`false` (also `yes`/`no`, `on`/`off`, `1`/`0`);
 * - a list: its number or label, as `tbx sbsar inputs` shows them;
 * - a vector or colour (`float3`, `float4`, `integer2`, ...): that many numbers
 *   separated by commas (`0.2,0.4,0.1`), or `#rrggbb` / `#rrggbbaa` for a colour;
 * - text: as it is.
 */

import { editableInputs, fromHex, rangeOf, type EngineInput, type Graph, type Value, type Values } from "./graph.js";

/** What one `--set` or `--values` entry can be before it is checked. */
export type RawValue = string | number | boolean | (string | number)[];

export interface ParamProblem {
  name: string;
  message: string;
  /** The archive has no such parameter (as opposed to a value that does not fit). */
  missing?: boolean;
}

const TRUE = new Set(["true", "yes", "on", "1"]);
const FALSE = new Set(["false", "no", "off", "0"]);

/** The edit distance between two short words. */
function distance(a: string, b: string): number {
  let row = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const next = [i];
    for (let j = 1; j <= b.length; j++) next[j] = Math.min(row[j] + 1, next[j - 1] + 1, row[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    row = next;
  }
  return row[b.length];
}

/** The candidates most like `name` (substrings first, then near spellings), at most `limit`. */
export function closeMatches(name: string, candidates: string[], limit = 5): string[] {
  const wanted = name.toLowerCase();
  const scored = candidates.map((candidate) => {
    const lower = candidate.toLowerCase();
    const near = distance(wanted, lower);
    const contained = wanted.length >= 3 && (lower.includes(wanted) || wanted.includes(lower));
    return { candidate, score: contained ? 0 : near, ok: contained || near <= Math.max(2, Math.floor(Math.max(wanted.length, lower.length) / 3)) };
  });
  return scored.filter((item) => item.ok).sort((a, b) => a.score - b.score || a.candidate.localeCompare(b.candidate)).slice(0, limit).map((item) => item.candidate);
}

/** `name=value` split at the first `=`. */
export function splitSetting(text: string): { name: string; value: string } | undefined {
  const at = text.indexOf("=");
  const name = at > 0 ? text.slice(0, at).trim() : "";
  return name ? { name, value: text.slice(at + 1).trim() } : undefined;
}

/** The settings of a `--values` file: a JSON object of name → value. */
export function parseValuesFile(text: string, where = "the values file"): Record<string, RawValue> {
  let doc: unknown;
  try { doc = JSON.parse(text.replace(/^﻿/, "")); } catch (failure) { throw new Error(`${where} is not valid JSON (${failure instanceof Error ? failure.message : String(failure)}).`); }
  if (!doc || typeof doc !== "object" || Array.isArray(doc)) throw new Error(`${where} must hold one JSON object of parameter name → value.`);
  const out: Record<string, RawValue> = {};
  for (const [name, value] of Object.entries(doc as Record<string, unknown>)) {
    const ok = typeof value === "string" || typeof value === "number" || typeof value === "boolean" || (Array.isArray(value) && value.every((item) => typeof item === "number" || typeof item === "string"));
    if (!ok) throw new Error(`${where}: "${name}" must be a number, text, true/false or a list of numbers.`);
    out[name] = value as RawValue;
  }
  return out;
}

const components = (type: string): number => /^(?:float|integer)([234])$/.exec(type)?.[1] ? Number(/([234])$/.exec(type)![1]) : 1;
const num = (value: unknown): number => (typeof value === "number" ? value : typeof value === "string" && value.trim() !== "" ? Number(value.trim()) : NaN);
const showValue = (value: unknown) => (Array.isArray(value) ? value.join(",") : String(value));

/** The label/value choices of a list input, as text for a message. */
export function describeChoices(input: EngineInput): string {
  return input.enumValues.map((choice) => `${choice.value} = ${choice.label}`).join(", ");
}

/** Check `raw` against `input` and return the value to send, or why it does not fit. */
export function coerceInput(input: EngineInput, raw: RawValue): { value: Value } | { problem: string } {
  const type = input.type;
  const widget = input.guiWidget;
  if (type === "image") return { problem: "is an image input; it cannot be set from the command line" };
  if (input.enumValues.length && (widget === "Combobox" || widget === "")) {
    const text = showValue(raw).trim();
    const byValue = input.enumValues.find((choice) => String(choice.value) === text);
    const byLabel = input.enumValues.find((choice) => choice.label.toLowerCase() === text.toLowerCase());
    const found = byValue ?? byLabel;
    return found ? { value: found.value } : { problem: `"${text}" is not one of ${describeChoices(input)}` };
  }
  if (widget === "Togglebutton" || type === "boolean") {
    const text = showValue(raw).trim().toLowerCase();
    if (TRUE.has(text)) return { value: 1 };
    if (FALSE.has(text)) return { value: 0 };
    return { problem: `"${showValue(raw)}" is not true or false` };
  }
  if (type === "string") return { value: typeof raw === "string" ? raw : showValue(raw) };
  const count = components(type);
  if (count > 1) {
    const text = typeof raw === "string" ? raw.trim() : undefined;
    if (text?.startsWith("#")) {
      if (!/^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(text)) return { problem: `"${text}" is not a colour like #rrggbb` };
      if (!/^float[34]$/.test(type)) return { problem: `a colour like ${text} fits a float3 or float4 input, not ${type}` };
      const rgb = fromHex(text.slice(0, 7), [0, 0, 0]);
      const alpha = text.length === 9 ? parseInt(text.slice(7), 16) / 255 : 1;
      const full = type === "float4" ? [...rgb, alpha] : rgb;
      return { value: full };
    }
    const parts = Array.isArray(raw) ? raw.map(num) : typeof raw === "string" ? raw.split(",").map(num) : [num(raw)];
    if (parts.length !== count || parts.some((part) => !Number.isFinite(part))) {
      return { problem: `"${showValue(raw)}" must be ${count} numbers separated by commas (type ${type})` };
    }
    if (type.startsWith("integer") && parts.some((part) => !Number.isInteger(part))) return { problem: `"${showValue(raw)}" must be whole numbers (type ${type})` };
    return { value: parts };
  }
  const value = typeof raw === "boolean" ? Number(raw) : num(Array.isArray(raw) ? raw[0] : raw);
  if (!Number.isFinite(value)) return { problem: `"${showValue(raw)}" is not a number` };
  if (type === "integer" && !Number.isInteger(value)) return { problem: `${value} is not a whole number (type integer)` };
  const range = input.identifier.startsWith("$") ? undefined : typeof input.minValue === "number" && typeof input.maxValue === "number" && input.maxValue > input.minValue ? rangeOf(input) : undefined;
  if (range && (value < range.min || value > range.max)) {
    return { problem: `${value} is ${value < range.min ? "below" : "above"} the ${value < range.min ? "minimum" : "maximum"} (allowed ${range.min} to ${range.max})` };
  }
  return { value };
}

export interface AppliedParameters {
  values: Values;
  problems: ParamProblem[];
  /** Names left out because the archive lacks them and `ignoreMissing` was given. */
  ignored: string[];
}

/** Lay `given` over `base` (a complete set of values) for `graph`. Every problem is collected, none stops the others. */
export function applyParameters(graph: Graph, base: Values, given: Record<string, RawValue>, options: { ignoreMissing?: boolean } = {}): AppliedParameters {
  const editable = editableInputs(graph);
  const names = editable.map((input) => input.identifier);
  const values: Values = { ...base };
  const problems: ParamProblem[] = [];
  const ignored: string[] = [];
  for (const [name, raw] of Object.entries(given)) {
    const exact = editable.find((item) => item.identifier === name);
    const loose = exact ?? (editable.filter((item) => item.identifier.toLowerCase() === name.toLowerCase()).length === 1 ? editable.find((item) => item.identifier.toLowerCase() === name.toLowerCase()) : undefined);
    if (!loose) {
      const known = graph.inputs.find((item) => item.identifier === name);
      if (known) {
        problems.push({ name, message: `"${name}" is set by Toybox itself (the size with --size; the normal map is always OpenGL; the output switches are not choices)` });
        continue;
      }
      if (options.ignoreMissing) { ignored.push(name); continue; }
      const close = closeMatches(name, names);
      problems.push({ name, missing: true, message: `has no parameter "${name}"${close.length ? `; did you mean ${close.join(", ")}?` : "; `tbx sbsar inputs <file>` lists its parameters"}` });
      continue;
    }
    const result = coerceInput(loose, raw);
    if ("problem" in result) problems.push({ name, message: `${loose.identifier}: ${result.problem}` });
    else values[loose.identifier] = result.value;
  }
  return { values, problems, ignored };
}

/** Parameter values written as `name=value` texts, in order (a later one wins). */
export function settingsFromTexts(texts: string[]): Record<string, RawValue> {
  const out: Record<string, RawValue> = {};
  for (const text of texts) {
    const parts = splitSetting(text);
    if (!parts) throw new Error(`"${text}" is not name=value (for example --set joint_height=0.3).`);
    out[parts.name] = parts.value;
  }
  return out;
}

/** Throws, listing the archive's presets, when `graph` has no preset called `label`. */
export function checkPreset(graph: Graph, label: string): void {
  if (graph.presets.some((preset) => preset.label === label)) return;
  const known = graph.presets.map((preset) => preset.label);
  const close = closeMatches(label, known);
  throw new Error(`has no preset named "${label}"${close.length ? `; did you mean ${close.join(", ")}?` : ""} (${known.length ? `it has: ${known.join(", ")}` : "it has none"})`);
}

type Reader = { readBytes(root: string, path: string): Promise<Uint8Array> };

/** Read a `--values` file (an absolute path) and parse it. */
export async function loadValuesFile(fs: Reader, file: string): Promise<Record<string, RawValue>> {
  const at = Math.max(file.lastIndexOf("/"), file.lastIndexOf("\\"));
  const head = file.slice(0, at);
  // The folder is the source root; the root of a drive or of the system keeps its separator.
  const dir = at < 0 ? "." : head === "" || /^[A-Za-z]:$/.test(head) ? file.slice(0, at + 1) : head;
  let bytes: Uint8Array;
  try { bytes = await fs.readBytes(dir, file.slice(at + 1)); } catch (failure) { throw new Error(`cannot read ${file}: ${failure instanceof Error ? failure.message : String(failure)}`); }
  return parseValuesFile(new TextDecoder().decode(bytes), file);
}

/** `--values` and `--set` together: the file first, then each `--set` over it. */
export async function givenParameters(fs: Reader, valuesFile: unknown, settings: unknown): Promise<Record<string, RawValue>> {
  const fromFile = typeof valuesFile === "string" && valuesFile ? await loadValuesFile(fs, valuesFile) : {};
  const texts = Array.isArray(settings) ? settings.map(String) : typeof settings === "string" && settings ? [settings] : [];
  return { ...fromFile, ...settingsFromTexts(texts) };
}
