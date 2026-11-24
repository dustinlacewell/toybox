/**
 * The rules behind the substance viewer and the parameter editor, free of React
 * and of the engine so they are unit-tested directly: which parameters show
 * (search, groups, visibility), which differ from the preset, what the editor
 * starts from for each target, and the names a new material may take.
 */

import { cleanName, editableInputs, presetValues, readMeta, sameValue, sizeOf, type EngineInput, type Graph, type Values } from "./graph.js";
import { pixels } from "./describe.js";
import { rawId } from "./substance.js";

// --- parameters ------------------------------------------------------------------

/** `joint_height` → `Joint height`: an identifier as a label, for when the archive's own labels are not at hand. */
export function prettyIdentifier(identifier: string): string {
  if (identifier === "$randomseed") return "Random seed";
  const words = identifier.replace(/^\$/, "").replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/[_\s]+/g, " ").trim().toLowerCase();
  return words ? words[0].toUpperCase() + words.slice(1) : identifier;
}

/** The label a parameter is shown with. */
export const labelOf = (input: EngineInput): string => (input.identifier === "$randomseed" ? "Random seed" : input.label || prettyIdentifier(input.identifier));

/** Whether a parameter answers a search: its label, identifier or group contains every word of `query`. */
export function matchesQuery(input: EngineInput, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const text = `${labelOf(input)} ${input.identifier} ${input.guiGroup}`.toLowerCase();
  return words.every((word) => text.includes(word));
}

export interface ParameterGroup {
  /** The archive's group name; "" for parameters in no group. */
  group: string;
  inputs: EngineInput[];
}

/** The parameters to show, grouped as the archive groups them: ungrouped first, then each group in the order it
 *  first appears. Only those the archive currently shows (`visibleIf`, which the engine re-evaluates after each
 *  change), that have a value, and that match `query`. */
export function parameterGroups(graph: Graph, values: Values, query = ""): ParameterGroup[] {
  const inputs = editableInputs(graph).filter((input) => input.visibleIf && values[input.identifier] !== undefined && matchesQuery(input, query));
  const order = [...new Set(inputs.map((input) => input.guiGroup))].sort((a, b) => Number(!!a) - Number(!!b));
  return order.map((group) => ({ group, inputs: inputs.filter((input) => input.guiGroup === group) }));
}

/** The parameters whose value differs from `base` (the chosen preset's values, or the defaults). */
export function changedFrom(values: Values, base: Values): Set<string> {
  return new Set(Object.keys(values).filter((key) => key in base && !sameValue(values[key], base[key])));
}

// --- what is edited --------------------------------------------------------------

/** What the editor is set to: graph, preset, every parameter's value and texture size (as the engine counts it). */
export interface EditorState {
  graph: number;
  preset: string | null;
  values: Values;
  size: number;
}

/** The record of a material the editor can start from. */
export interface MaterialRecord {
  graph: number;
  preset: string | null;
  inputs: Values;
  size: number;
}

/** A material's record, from its `origin.meta`. */
export function recordOf(meta: unknown): MaterialRecord | undefined {
  const parsed = readMeta(meta);
  return parsed && { graph: parsed.graph, preset: parsed.preset, inputs: parsed.inputs, size: parsed.size };
}

/** What the editor starts from for a material (its preset with its changes on top), or for a new material (the
 *  graph's defaults at `size`). A record of another graph is not this graph's: its values are not laid over this
 *  one, which starts from its defaults. A preset the archive no longer has falls back to the defaults. */
export function startingState(graph: Graph, record: MaterialRecord | undefined, size: number): EditorState {
  if (!record || record.graph !== graph.index) return { graph: graph.index, preset: null, values: presetValues(graph, null), size };
  const preset = record.preset && graph.presets.some((item) => item.label === record.preset) ? record.preset : null;
  const values = presetValues(graph, preset);
  for (const [key, value] of Object.entries(record.inputs)) if (key in values) values[key] = value;
  return { graph: graph.index, preset, values, size: sizeOf(record.size) };
}

/** Whether two editor states render the same. */
export function sameState(a: EditorState | undefined, b: EditorState | undefined): boolean {
  if (!a || !b) return a === b;
  if (a.graph !== b.graph || a.preset !== b.preset || a.size !== b.size) return false;
  const keys = new Set([...Object.keys(a.values), ...Object.keys(b.values)]);
  return [...keys].every((key) => sameValue(a.values[key], b.values[key]));
}

// --- the materials of a substance ------------------------------------------------

/** What the viewer edits: a new material, or one made from the substance (by raw id). */
export type Target = { kind: "new" } | { kind: "material"; id: string };

export const NEW_TARGET: Target = { kind: "new" };

export const sameTarget = (a: Target, b: Target): boolean => (a.kind === "new" ? b.kind === "new" : b.kind === "material" && rawId(a.id) === rawId(b.id));

/** The target a viewer opens with: the material asked for (from a material's "Open substance") when it is one of
 *  the substance's, else the first material, else a new one. */
export function initialTarget(children: { id: string }[], requested?: string): Target {
  const asked = requested ? children.find((child) => rawId(child.id) === rawId(requested)) : undefined;
  const chosen = asked ?? children[0];
  return chosen ? { kind: "material", id: rawId(chosen.id) } : NEW_TARGET;
}

/** The materials of a substance in the order the viewer lists them: by name. */
export function sortByName<T extends { name: string }>(items: T[]): T[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }));
}

/** A material's line under its name: `Green · 1024 px`. */
export function materialCaption(meta: unknown): string {
  const record = readMeta(meta);
  if (!record) return "";
  const changed = Object.keys(record.inputs).length;
  return [record.preset ?? "Defaults", ...(changed ? [`${changed} changed`] : []), `${pixels(record.size)} px`].join(" · ");
}

/** `base`, or `base 2`, `base 3`, ... — the first that none of `taken` has (ignoring case). */
export function uniqueName(base: string, taken: string[]): string {
  const used = new Set(taken.map((name) => name.toLowerCase()));
  const clean = cleanName(base);
  let name = clean;
  for (let copy = 2; used.has(name.toLowerCase()); copy++) name = `${clean} ${copy}`;
  return name;
}

/** The name a new material is offered: the substance's name, with the preset when one is chosen, not yet taken. */
export function defaultNewName(substance: string, preset: string | null, taken: string[]): string {
  return uniqueName(preset ? `${substance} - ${preset}` : substance, taken);
}

/** Why `name` cannot name a new material of the substance, or undefined when it can. `taken` are the names of its
 *  materials: saving under one of them would replace that material, which "Update" is for. */
export function checkNewName(name: string, taken: string[]): string | undefined {
  if (!name.trim()) return "Enter a name for the new material.";
  const clean = cleanName(name);
  if (taken.some((other) => other.toLowerCase() === clean.toLowerCase())) return `"${clean}" is already a material of this substance. Choose another name, or update that material.`;
  return undefined;
}

// --- what the viewer edits, step by step -----------------------------------------

/** The viewer's editing state: the target, the graph shown, and the editor's values with the target they were
 *  loaded for. Kept by `editReducer`, a pure function, so every step is tested without React. */
export interface EditState {
  target?: Target;
  /** The graph the editor shows (a material's own graph, or the one chosen for a new material). */
  graph: number;
  editor?: EditorState;
  /** The target whose starting values `editor` was loaded from (then edited). "Update" may only be offered when
   *  this is the target: otherwise the values in the editor belong to something else. */
  loadedFor?: Target;
  /** The raw id of a material just saved as new and made the target, until the materials list has it. */
  pending?: string;
  /** The chosen material's record could not be read: it is shown from the defaults of its first graph. */
  unknown?: boolean;
}

export type EditAction =
  /** Choose what to edit. `record` is the chosen material's (undefined for a new one, or when it cannot be read). */
  | { type: "select"; target: Target; record?: MaterialRecord }
  /** The person chose another graph. A material is rendered from its own graph, so with a material chosen this
   *  turns to a new material of that graph instead of silently converting the material. */
  | { type: "graph"; graph: number }
  /** The starting values for the current target and graph are known: the editor starts from them if it is empty. */
  | { type: "load"; baseline: EditorState }
  /** The person changed something. */
  | { type: "edit"; editor: EditorState }
  /** Back to the starting values of the current target. */
  | { type: "revert"; baseline: EditorState }
  /** A material was saved as new (raw or qualified id): it becomes the target at once, with the editor's values,
   *  which are what was saved, even before the list shows it. */
  | { type: "savedNew"; id: string }
  /** The materials list was read again (their raw or qualified ids). */
  | { type: "children"; ids: string[] };

export const initialEdit: EditState = { graph: 0 };

export function editReducer(state: EditState, action: EditAction): EditState {
  switch (action.type) {
    case "select": {
      const graph = action.target.kind === "material" ? action.record?.graph ?? 0 : state.graph;
      return { target: action.target, graph, unknown: action.target.kind === "material" && !action.record };
    }
    case "graph":
      if (action.graph === state.graph) return state;
      return { target: state.target?.kind === "material" ? NEW_TARGET : state.target, graph: action.graph };
    case "load":
      if (state.editor || !state.target || action.baseline.graph !== state.graph) return state;
      return { ...state, editor: action.baseline, loadedFor: state.target };
    case "edit":
      return state.editor ? { ...state, editor: action.editor } : state;
    case "revert":
      if (!state.target || action.baseline.graph !== state.graph) return state;
      return { ...state, editor: action.baseline, loadedFor: state.target };
    case "savedNew": {
      const target: Target = { kind: "material", id: rawId(action.id) };
      return { ...state, target, loadedFor: target, pending: target.id, unknown: false };
    }
    case "children": {
      const ids = new Set(action.ids.map(rawId));
      if (state.pending && ids.has(state.pending)) return { ...state, pending: undefined };
      // A target material that was deleted meanwhile leaves the editor on a new material, keeping its values.
      if (state.target?.kind === "material" && state.target.id !== state.pending && !ids.has(state.target.id)) return { ...state, target: NEW_TARGET, loadedFor: NEW_TARGET, unknown: false };
      return state;
    }
  }
}

/** Whether "Update <material>" may be offered: a material is the target, it is listed, and the editor's values
 *  were loaded for it. */
export function canUpdate(state: EditState): boolean {
  return state.target?.kind === "material" && !state.pending && !!state.editor && !!state.loadedFor && sameTarget(state.loadedFor, state.target);
}
