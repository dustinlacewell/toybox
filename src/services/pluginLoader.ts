/**
 * Load a discovered plugin's entry module and execute it.
 *
 * Modules are imported from the custom `plugin` protocol origin (served by the
 * Rust protocol handler, jailed to the plugins dir). A real origin — unlike a
 * blob/data URL — is subject to the document import map, so a plugin's bare
 * `import "three"` / `import "toybox"` resolves to the app's shared chunks (one
 * instance across the app/plugin boundary). `@vite-ignore` keeps Vite from
 * trying to resolve the runtime URL at build time.
 *
 * The scheme is platform-specific: WebView2 (Windows) and Android expose the
 * custom protocol as `http://plugin.localhost/...`, while macOS/iOS/Linux use
 * `plugin://localhost/...`. We derive the correct origin from `convertFileSrc`
 * rather than hardcoding, then append the (un-encoded) plugin path ourselves —
 * `convertFileSrc` percent-encodes `/`, which would break the path segments.
 */

import { convertFileSrc } from "@tauri-apps/api/core";

import type { ComponentType } from "react";

import type { ExporterPlugin, SlotComponentProps } from "@ldlework/toybox-sdk";
import type { PluginManifestDto } from "./tauriApi";

export interface LoadedPlugin {
  manifest: PluginManifestDto;
  plugin: ExporterPlugin;
}

/** The protocol root, e.g. `http://plugin.localhost/` (Windows) or
 *  `plugin://localhost/` (macOS/Linux). `convertFileSrc("")` yields the origin
 *  with an encoded-empty path; strip back to the trailing slash. */
function protocolRoot(): string {
  const base = convertFileSrc("", "plugin");
  // base looks like "http://plugin.localhost/" or with a trailing encoded path;
  // keep through the host's trailing slash.
  const m = base.match(/^[a-z]+:\/\/[^/]+\//i);
  return m ? m[0] : base;
}

/** The protocol URL for a file (`rel`, plugin-dir-relative) under plugin `id`,
 *  path segments preserved (`convertFileSrc` would percent-encode the `/`). */
export function pluginUrl(id: string, rel: string): string {
  const path = `${id}/${rel}`.split("/").map(encodeURIComponent).join("/");
  return protocolRoot() + path;
}

/** Dynamically import an exporter plugin's entry module (default-exports `run`).
 *  Throws on a malformed module so the registry can mark it disabled-with-reason. */
export async function loadPlugin(m: PluginManifestDto): Promise<LoadedPlugin> {
  const mod = await import(/* @vite-ignore */ pluginUrl(m.id, m.entry));
  const plugin = mod.default;
  if (!plugin || typeof plugin.run !== "function") {
    throw new Error(`plugin '${m.id}' has no default export with a run() method`);
  }
  return { manifest: m, plugin };
}

/** Dynamically import a slot UI module's default-exported React component. */
export async function loadSlotComponent(
  id: string,
  rel: string,
): Promise<ComponentType<SlotComponentProps<unknown>>> {
  const mod = await import(/* @vite-ignore */ pluginUrl(id, rel));
  if (typeof mod.default !== "function") {
    throw new Error(`plugin '${id}' slot module '${rel}' has no default export`);
  }
  return mod.default as ComponentType<SlotComponentProps<unknown>>;
}
