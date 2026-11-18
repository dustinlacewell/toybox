/**
 * Finding the Models plugin's `model.api` service. Plugins never import each
 * other at run time, so the service reaches this plugin through the extension
 * point the Models plugin contributes to itself. Only a value contributed by
 * the Models plugin and shaped like the service counts. Pure.
 */

import type { ModelApi } from "com.toybox.model-core";

/** The extension point the Models plugin publishes its service on. */
export const MODEL_API = "model.api";
/** The plugin this one builds on. */
export const MODEL_CORE = "com.toybox.model-core";

export interface Contribution {
  pluginId: string;
  value: unknown;
}

const METHODS = ["formats", "lookingFor", "scan", "convert", "hasAnimationClips"] as const;

export function isModelApi(value: unknown): value is ModelApi {
  const v = value as Record<string, unknown> | null;
  return !!v && typeof v === "object" && METHODS.every((name) => typeof v[name] === "function");
}

/** The service of the Models plugin, or undefined while it is not available. */
export function findModelApi(contributions: Contribution[]): ModelApi | undefined {
  return contributions.find((c) => c.pluginId === MODEL_CORE && isModelApi(c.value))?.value as ModelApi | undefined;
}
