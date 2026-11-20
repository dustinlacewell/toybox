/**
 * The context menu of a tile: which items it acts on, and its entries. A right-click on a selected tile acts on
 * the whole selection; on any other tile it acts on that tile alone and leaves the selection as it is.
 */

export type AssetAction = "open" | "collect" | "tag" | "copy" | "move" | "reveal" | "export" | "clear" | "delete";

export type AssetMenuItem = { kind: "item"; id: AssetAction; label: string; tone?: "danger"; disabled?: boolean; reason?: string };
export type AssetMenuEntry = { kind: "heading"; label: string } | { kind: "separator" } | AssetMenuItem;

export interface AssetMenuInput {
  /** The ids acted on; `single` names the one item when there is exactly one. */
  target: { ids: string[]; single?: { name: string; remote: boolean } };
  selectionSize: number;
  offerTransfer: boolean;
  /** Why the items cannot be changed in their library (Tag, Move and Delete), when they cannot. */
  sourceLocked?: string;
}

/** The ids a right-click on `clickedId` acts on: the selection (in its order) when the item is in it, else the item. */
export function menuTarget(clickedId: string, selection: ReadonlySet<string>): string[] {
  return selection.has(clickedId) ? [...selection] : [clickedId];
}

/** A heading naming the target, then the groups of items that apply to it, separated. */
export function assetMenu(input: AssetMenuInput): AssetMenuEntry[] {
  const { target: { ids, single }, selectionSize, offerTransfer } = input;
  const groups: AssetMenuItem[][] = [
    single ? [item("open", "Open")] : [],
    [item("collect", "Add to collection"), item("tag", "Tag…", input.sourceLocked), ...(offerTransfer ? [item("copy", "Copy to library…"), item("move", "Move to library…", input.sourceLocked)] : [])],
    [...(single && !single.remote ? [item("reveal", "Show in folder")] : []), item("export", "Export")],
    [...(selectionSize > 0 ? [item("clear", "Clear selection")] : []), { ...item("delete", "Delete", input.sourceLocked), tone: "danger" as const }],
  ];
  return [{ kind: "heading", label: single ? single.name : `${ids.length} selected` }, ...separated(groups)];
}

function item(id: AssetAction, label: string, blocked?: string): AssetMenuItem {
  return blocked ? { kind: "item", id, label, disabled: true, reason: blocked } : { kind: "item", id, label };
}

function separated(groups: AssetMenuItem[][]): AssetMenuEntry[] {
  return groups.filter((group) => group.length).flatMap((group, index) => index ? [{ kind: "separator" as const }, ...group] : group);
}
