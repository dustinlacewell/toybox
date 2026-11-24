/**
 * Which plugin an imported item came from. The host stamps the importing
 * plugin's id on every entry it commits; the plugin may add its own record
 * (`meta`) but cannot claim another plugin's items. The one exception is a
 * plugin that depends on the importer (a format plugin of the model importer):
 * the importer may name it, because it did the work for that format. The
 * item an entry was made from (`derivedFrom`) is kept as the plugin gave it. Pure.
 */

import type { SeedEntryInput } from "@ldlework/toybox-sdk";

/** @param attributable ids of the plugins the importer may name instead of
 *  itself: the switched-on plugins that directly need it. */
export function withOrigin(entry: SeedEntryInput, pluginId: string, attributable: readonly string[] = []): SeedEntryInput {
  if (!pluginId) return entry;
  const meta = entry.origin?.meta;
  const files = entry.origin?.files;
  const derivedFrom = entry.origin?.derivedFrom;
  const named = entry.origin?.plugin;
  const plugin = named && attributable.includes(named) ? named : pluginId;
  const result = { ...entry, origin: { plugin, ...(meta && Object.keys(meta).length ? { meta } : {}), ...(files?.length ? { files } : {}), ...(derivedFrom ? { derivedFrom } : {}) } };
  if ("variants" in entry && entry.variants) {
    return { ...result, variants: entry.variants.map((variant) => withOrigin(variant, pluginId, attributable)) } as SeedEntryInput;
  }
  return result as SeedEntryInput;
}
