/**
 * What the Substance engine reports about a loaded archive, and the rules for
 * turning it into controls, parameter values and a material's maps. Pure — no
 * host, DOM or engine calls — so the rules are unit-tested directly.
 */

import type { MapChannel, MapRole } from "@ldlework/toybox-sdk";

/** A parameter value: a number, a vector, or text. */
export type Value = number | number[] | string;
export type Values = Record<string, Value>;

export interface EngineInput {
  id: number;
  identifier: string;
  label: string;
  /** `float`, `float3`, `integer`, `integer2`, `string`, … */
  type: string;
  /** `Slider`, `Color`, `Togglebutton`, `Combobox`, `Angle`, `NoWidget`, … */
  guiWidget: string;
  guiGroup: string;
  guiDescription: string;
  value: Value;
  defaultValue: Value;
  minValue: Value;
  maxValue: Value;
  sliderStep: number;
  visibleIf: boolean;
  enumValues: { value: number; label: string }[];
  labelTrue: string;
  labelFalse: string;
}

export interface EngineOutput {
  identifier: string;
  label: string;
  /** What the output is used as, e.g. `baseColor`, `normal`. */
  usage: string;
}

export interface EnginePreset {
  label: string;
  /** Raw text values by input identifier. */
  inputs: Record<string, string>;
}

export interface Graph {
  index: number;
  label: string;
  inputs: EngineInput[];
  outputs: EngineOutput[];
  presets: EnginePreset[];
}

const record = (value: unknown): Record<string, unknown> => (value && typeof value === "object" ? (value as Record<string, unknown>) : {});
const text = (value: unknown): string => (typeof value === "string" ? value : "");
const isValue = (value: unknown): value is Value =>
  typeof value === "number" || typeof value === "string" || (Array.isArray(value) && value.every((item) => typeof item === "number"));

function readInput(raw: unknown): EngineInput | undefined {
  const input = record(raw);
  if (typeof input.identifier !== "string" || typeof input.id !== "number" || typeof input.type !== "string") return undefined;
  const value = isValue(input.value) ? input.value : 0;
  const enums = Array.isArray(input.enumValues) ? input.enumValues : [];
  return {
    id: input.id,
    identifier: input.identifier,
    label: text(input.label) || input.identifier,
    type: input.type,
    guiWidget: text(input.guiWidget),
    guiGroup: text(input.guiGroup),
    guiDescription: text(input.guiDescription),
    value,
    defaultValue: isValue(input.defaultValue) ? input.defaultValue : value,
    minValue: isValue(input.minValue) ? input.minValue : 0,
    maxValue: isValue(input.maxValue) ? input.maxValue : 0,
    sliderStep: typeof input.sliderStep === "number" ? input.sliderStep : 0,
    visibleIf: input.visibleIf !== false,
    enumValues: enums.map((item) => record(item)).filter((item) => typeof item.first === "number")
      .map((item) => ({ value: item.first as number, label: text(item.second) || String(item.first) })),
    labelTrue: text(input.labelTrue),
    labelFalse: text(input.labelFalse),
  };
}

/** The inputs in a reply to an update: the engine re-evaluates which are visible. */
export function readInputs(raw: unknown): EngineInput[] {
  return (Array.isArray(raw) ? raw : []).map(readInput).filter((input): input is EngineInput => !!input);
}

/** Take which inputs are visible from an update reply, and nothing else. After
 *  an update the engine reports each input's current value as its "default",
 *  so the real defaults — known only from the first load — must be kept. */
export function withVisibility(inputs: EngineInput[], refreshed: EngineInput[]): EngineInput[] {
  const visible = new Map(refreshed.map((input) => [input.identifier, input.visibleIf]));
  return inputs.map((input) => {
    const now = visible.get(input.identifier);
    return now === undefined || now === input.visibleIf ? input : { ...input, visibleIf: now };
  });
}

/** A preset's values from its `<sbspreset>` text. */
export function presetInputs(xml: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const element of xml.matchAll(/<presetinput\b([^>]*)>/g)) {
    const attribute = (name: string) => new RegExp(`\\b${name}="([^"]*)"`).exec(element[1])?.[1];
    const identifier = attribute("identifier");
    const value = attribute("value");
    if (identifier !== undefined && value !== undefined) out[identifier] = value;
  }
  return out;
}

/** The graphs of an engine load reply. Read defensively: it is another
 *  program's format. */
export function readGraphs(reply: unknown): Graph[] {
  const graphs = record(record(reply).data).graphs;
  return (Array.isArray(graphs) ? graphs : []).map((raw, index) => {
    const graph = record(raw);
    const outputs = (Array.isArray(graph.outputs) ? graph.outputs : []).map((item) => record(item))
      .filter((output) => typeof output.identifier === "string")
      .map((output) => ({
        identifier: output.identifier as string,
        label: text(output.label) || (output.identifier as string),
        usage: text(output.defaultChannelUse) || text(Array.isArray(output.channelUse) ? output.channelUse[0] : ""),
      }));
    // Presets shipped in the archive. The engine also lists a working copy of
    // its own state (not embedded), which is not a choice to offer.
    const presets = (Array.isArray(graph.presets) ? graph.presets : []).map((item) => record(item))
      .filter((preset) => preset.embedded === true && typeof preset.label === "string" && preset.label)
      .map((preset) => ({ label: preset.label as string, inputs: presetInputs(text(preset.value)) }));
    return { index: typeof graph.index === "number" ? graph.index : index, label: text(graph.label) || `Graph ${index + 1}`, inputs: readInputs(graph.inputs), outputs, presets };
  });
}

// --- values ------------------------------------------------------------------

/** Turn text (a preset stores every value as text) into the form the engine
 *  expects for an input of `type`. */
export function coerce(type: string, raw: Value): Value {
  if (typeof raw !== "string") return raw;
  const parts = raw.split(",").map((part) => Number(part.trim()));
  const numeric = parts.every((part) => Number.isFinite(part));
  if (/^(float|integer)[234]$/.test(type)) return numeric ? parts : raw;
  if (type === "float" || type === "integer" || type === "boolean") return numeric && parts.length === 1 ? parts[0] : raw;
  return raw;
}

export function sameValue(a: Value | undefined, b: Value | undefined): boolean {
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((item, index) => Math.abs(item - b[index]) < 1e-6);
  if (typeof a === "number" && typeof b === "number") return Math.abs(a - b) < 1e-6;
  return a === b;
}

/** Inputs the plugin sets itself and never offers: the texture size, and the
 *  normal-map convention (always OpenGL, which is what the library stores). */
const RESERVED = new Set(["$outputsize", "normal_format"]);
/** Switches for which outputs exist. The plugin renders every output and keeps
 *  the ones a material uses, so they are not choices to offer either. */
const isChannelSwitch = (input: EngineInput) => /^channel_/.test(input.identifier) && input.guiWidget === "Togglebutton";

/** The inputs a person can change, in the archive's own order. */
export function editableInputs(graph: Graph): EngineInput[] {
  return graph.inputs.filter((input) => !RESERVED.has(input.identifier) && !isChannelSwitch(input)
    && (input.identifier === "$randomseed" || (!input.identifier.startsWith("$") && input.guiWidget !== "NoWidget")));
}

/** Every editable input at its default, overlaid with a preset's values. */
export function presetValues(graph: Graph, preset?: string | null): Values {
  const editable = editableInputs(graph);
  const values: Values = Object.fromEntries(editable.map((input) => [input.identifier, input.defaultValue]));
  const chosen = preset ? graph.presets.find((item) => item.label === preset) : undefined;
  for (const input of editable) {
    const raw = chosen?.inputs[input.identifier];
    if (raw !== undefined) values[input.identifier] = coerce(input.type, raw);
  }
  return values;
}

/** The values that differ from `base` — what is worth recording. */
export function changedValues(values: Values, base: Values): Values {
  return Object.fromEntries(Object.entries(values).filter(([key, value]) => !sameValue(value, base[key])));
}

/** A slider's range. Substance leaves both ends at 0 for "unit range". */
export function rangeOf(input: EngineInput): { min: number; max: number; step: number } {
  const min = typeof input.minValue === "number" ? input.minValue : 0;
  const max = typeof input.maxValue === "number" ? input.maxValue : 0;
  const whole = input.type === "integer";
  if (max <= min) return whole ? { min: 0, max: 10, step: 1 } : { min: 0, max: 1, step: 0.01 };
  return { min, max, step: whole ? 1 : input.sliderStep > 0 ? input.sliderStep : (max - min) / 100 };
}

// --- colours -----------------------------------------------------------------

const channel = (value: number) => Math.round(Math.min(1, Math.max(0, value)) * 255).toString(16).padStart(2, "0");
/** `[r, g, b]` in 0–1 as `#rrggbb`. */
export const toHex = (value: Value): string => (Array.isArray(value) ? `#${channel(value[0] ?? 0)}${channel(value[1] ?? 0)}${channel(value[2] ?? 0)}` : "#000000");
/** `#rrggbb` as 0–1 components, keeping any further components (alpha) of `previous`. */
export function fromHex(hex: string, previous: Value): number[] {
  const match = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  const rest = Array.isArray(previous) ? previous.slice(3) : [];
  return match ? [...match.slice(1, 4).map((part) => parseInt(part, 16) / 255), ...rest] : Array.isArray(previous) ? previous : [0, 0, 0];
}

// --- outputs → material maps -------------------------------------------------

/** Which map each engine output becomes, by its usage or identifier. */
const ROLES: Record<string, MapRole> = {
  basecolor: "albedo", normal: "normal", roughness: "roughness", metallic: "metallic",
  ambientocclusion: "ao", height: "height", emissive: "emissive", opacity: "opacity",
};
/** Scalar maps come out as grey images; the library reads one channel. */
const SCALAR: Partial<Record<MapRole, MapChannel>> = { roughness: "r", metallic: "r", ao: "r", height: "r", opacity: "r" };
export const channelOf = (role: MapRole): MapChannel | undefined => SCALAR[role];

const key = (name: string) => name.toLowerCase().replace(/[^a-z]/g, "");

/** The role of each output a material uses. An archive without a base colour
 *  (a specular/glossiness one) contributes its diffuse output as colour. */
export function outputRoles(graph: Graph): { identifier: string; role: MapRole }[] {
  const found = new Map<MapRole, string>();
  for (const output of graph.outputs) {
    const role = ROLES[key(output.usage)] ?? ROLES[key(output.identifier)];
    if (role && !found.has(role)) found.set(role, output.identifier);
  }
  if (!found.has("albedo")) {
    const diffuse = graph.outputs.find((output) => key(output.usage) === "diffuse" || key(output.identifier) === "diffuse");
    if (diffuse) found.set("albedo", diffuse.identifier);
  }
  return [...found].map(([role, identifier]) => ({ identifier, role }));
}

/** The file the engine wrote for output `identifier`: it names files
 *  `<name>_<graph>_<identifier>.<ext>`. The longest matching identifier wins,
 *  so `height` is not taken for an output called `detail_height`. */
export function outputFile(files: string[], identifier: string, all: string[]): string | undefined {
  const stem = (file: string) => file.replace(/\.[^.]+$/, "").toLowerCase();
  const ends = (file: string, id: string) => stem(file).endsWith(`_${id.toLowerCase()}`);
  return files.find((file) => ends(file, identifier) && !all.some((other) => other.length > identifier.length && ends(file, other)));
}

// --- names -------------------------------------------------------------------

/** One safe path segment on every platform. */
export function cleanName(name: string): string {
  const cleaned = name.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").replace(/\s+/g, " ").replace(/^[. ]+|[. ]+$/g, "");
  return cleaned || "Material";
}

/** `ceramic_tiles_blue` + `Commander Green` → `ceramic_tiles_blue - Commander Green`. */
export const materialName = (stem: string, preset?: string | null): string => cleanName(preset ? `${stem} - ${preset}` : stem);

/** A stable id: the same source and name always give the same id, so importing
 *  a folder again updates its materials instead of duplicating them. */
export function materialId(pack: string, name: string): string {
  let hash = 0xcbf29ce484222325n;
  for (const char of `${pack}\0${name.toLowerCase()}`) hash = BigInt.asUintN(64, (hash ^ BigInt(char.codePointAt(0)!)) * 0x100000001b3n);
  return `uid://sbsar_${hash.toString(16)}`;
}

/** Texture sizes offered, as the engine counts them (a power of two). */
export const SIZES = [{ value: "9", label: "512" }, { value: "10", label: "1024" }, { value: "11", label: "2048" }, { value: "12", label: "4096" }];
/** The size a new material is rendered at when the *Default texture size* setting holds nothing usable: 1024.
 *  (The same value is the setting's `default` in the manifest.) */
export const DEFAULT_SIZE = 10;
/** `value` as a size. The fallback of 11 (2048) is for a material's record: one written before the size was
 *  recorded was rendered at 2048. Pass `DEFAULT_SIZE` when reading the setting. */
export const sizeOf = (value: unknown, fallback = 11): number => {
  const size = Number(value);
  return Number.isInteger(size) && size >= 5 && size <= 13 ? size : fallback;
};

/** What the plugin records on a material it made, to render it again. */
export interface SbsarMeta {
  /** File name of the archive, kept in the material's own folder. */
  sbsar: string;
  graph: number;
  preset: string | null;
  /** Values that differ from the preset (or from the defaults without one). */
  inputs: Values;
  size: number;
  /** A fingerprint of the archive's content (see `fingerprint` in `archive.ts`), when it was recorded. */
  archiveHash?: string;
  /** The label of the graph, when it is not the archive's first (the name of such a material says it). */
  graphLabel?: string;
}

export function readMeta(meta: unknown): SbsarMeta | undefined {
  const value = record(meta);
  if (typeof value.sbsar !== "string" || !value.sbsar || /[\\/]/.test(value.sbsar)) return undefined;
  const inputs = Object.fromEntries(Object.entries(record(value.inputs)).filter((entry): entry is [string, Value] => isValue(entry[1])));
  return { sbsar: value.sbsar, graph: typeof value.graph === "number" ? value.graph : 0, preset: typeof value.preset === "string" ? value.preset : null, inputs, size: sizeOf(value.size),
    ...(typeof value.archiveHash === "string" && value.archiveHash ? { archiveHash: value.archiveHash } : {}),
    ...(typeof value.graphLabel === "string" && value.graphLabel ? { graphLabel: value.graphLabel } : {}) };
}
