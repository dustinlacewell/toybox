/**
 * The scrolling list of materials, in the review dialog and in the drawer panel.
 * Shows the rows at `indexes`, in that order. By default it fills its flex
 * parent; give `maxHeight` to size it to its rows instead.
 */

import type { ReviewItem } from "../lib/edit.js";
import { MaterialRow } from "./MaterialRow.js";
import type { ThumbLoader } from "./thumbs.js";
import { materialGroups } from "../lib/variants.js";
import { variantInfo } from "@ldlework/toybox-sdk";

interface Props {
  items: ReviewItem[];
  /** Problems per item, aligned with `items`. */
  problems: string[][];
  /** Which items to list. */
  indexes: number[];
  /** The highlighted item; omit where nothing is selected. */
  selected?: number;
  /** Cap the list height (any CSS length); it scrolls past that. */
  maxHeight?: string;
  loader: ThumbLoader;
  onSelect?: (index: number) => void;
  /** Double-click on a row. */
  onOpen?: (index: number) => void;
  onInclude: (index: number, included: boolean) => void;
}

export function MaterialList({
  items,
  problems,
  indexes,
  selected,
  maxHeight,
  loader,
  onSelect,
  onOpen,
  onInclude,
}: Props) {
  const groups = materialGroups(items).filter((g) => g.some((i) => indexes.includes(i)));
  return (
    <div style={{ flex: 1, minHeight: 0, maxHeight, overflowY: "auto" }}>
      {groups.map((group) => {
        const i = group[0];
        return (
        <MaterialRow
          // Order is fixed while reviewing, so the index is a stable key.
          key={i}
          item={{ ...items[i], included: group.some((j) => items[j].included), draft: { ...items[i].draft, name: variantInfo(items[i].draft.name, "material").base } }}
          problems={group.flatMap((j) => problems[j])}
          selected={selected !== undefined && group.includes(selected)}
          loader={loader}
          onSelect={onSelect && (() => onSelect(i))}
          onOpen={onOpen && (() => onOpen(i))}
          onInclude={(included) => group.forEach((j) => onInclude(j, included))}
        />
      ); })}
    </div>
  );
}
