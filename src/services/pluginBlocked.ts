/**
 * The sentence shown for a plugin that is switched on but cannot load because
 * a plugin it needs is not usable. Pure: no host imports.
 */

export interface BlockedBy {
  id: string;
  reason: "missing" | "version" | "disabled" | "blocked" | "cycle";
  needs?: string;
  found?: string;
}

interface Named { id: string; name: string }

/** One sentence per blocked dependency, joined. A dependency is named by its
 *  display name when it is installed, else by its id. */
export function blockedSentence(blockedBy: BlockedBy[], installed: Named[]): string {
  const names = new Map(installed.map((plugin) => [plugin.id, plugin.name]));
  return blockedBy.map((block) => {
    const name = names.get(block.id) ?? block.id;
    switch (block.reason) {
      case "missing": return `Needs ${name}, which is not installed.`;
      case "disabled": return `Needs ${name}, which is turned off.`;
      case "blocked": return `Needs ${name}, which cannot load.`;
      case "cycle": return `Needs ${name}, and ${name} needs it back.`;
      case "version": {
        const found = block.found ? `; version ${block.found} is installed` : "";
        const caret = block.needs?.match(/^\^(\d+\.\d+\.\d+)$/);
        if (caret) return `Needs ${name} ${caret[1]} or compatible (${block.needs})${found}.`;
        return `Needs ${name}${block.needs ? ` ${block.needs}` : ""}${found}.`;
      }
      default: return `Needs ${name}, which cannot be used.`;
    }
  }).join(" ");
}
