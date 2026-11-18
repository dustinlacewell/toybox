/**
 * The `model.api` service: what other plugins (animations) may use of the model
 * importer without importing this plugin's code. It is built in this plugin's
 * entry module, so every call runs with this plugin's own file permissions, and
 * it asks for the formats on every call, so it follows plugins being turned on
 * and off. Pure orchestration over injected capabilities, so it is tested
 * without a window.
 */

import type { FsApi } from "@ldlework/toybox-sdk";
import type { ModelApi, ModelFile, ModelFormat } from "../api.js";
import { collectFormats, lookingFor, type Contribution } from "./formats.js";
import { findModels, materialize } from "./walk.js";

export function createModelApi(fs: FsApi, formatContributions: () => Contribution[]): ModelApi {
  const table = () => collectFormats(formatContributions());
  return {
    formats() {
      const seen = new Set<ModelFormat>();
      for (const { format } of table().claims.values()) seen.add(format);
      return [...seen].map(({ id, label, extensions }) => ({ id, label, extensions: [...extensions] }));
    },
    lookingFor: () => lookingFor(table()),
    scan: (sourceRoot) => findModels(fs, sourceRoot, table()),
    async convert(sourceRoot, libraryRoot, file: ModelFile, pack, category) {
      const result = await materialize({ fs, sourceRoot, libraryRoot, formats: table() }, file, pack, category);
      return "rejected" in result ? { rejected: result.rejected } : { file: result.file, warnings: result.warnings };
    },
    async hasAnimationClips(libraryRoot, pack, category, file) {
      try {
        const doc = JSON.parse(new TextDecoder().decode(await fs.readBytes(libraryRoot, `library/${pack}/${category}/${file}`)));
        return Array.isArray(doc.animations) && doc.animations.some((clip: { channels?: unknown[] }) => clip?.channels?.length);
      } catch {
        return false;
      }
    },
  };
}
