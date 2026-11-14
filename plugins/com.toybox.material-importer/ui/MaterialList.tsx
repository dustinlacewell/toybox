/**
 * The scrolling list of materials in the review dialog. Shows the rows at
 * `indexes`, in that order.
 */

import type { ReviewItem } from "./edit.js";
import { MaterialRow } from "./MaterialRow.js";
import type { ThumbLoader } from "./thumbs.js";

interface Props {
  items: ReviewItem[];
  /** Problems per item, aligned with `items`. */
  problems: string[][];
  /** Which items to list. */
  indexes: number[];
  selected: number;
  loader: ThumbLoader;
  onSelect: (index: number) => void;
  onInclude: (index: number, included: boolean) => void;
}

export function MaterialList({
  items,
  problems,
  indexes,
  selected,
  loader,
  onSelect,
  onInclude,
}: Props) {
  return (
    <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
      {indexes.map((i) => (
        <MaterialRow
          // Order is fixed while reviewing, so the index is a stable key.
          key={i}
          item={items[i]}
          problems={problems[i]}
          selected={i === selected}
          loader={loader}
          onSelect={() => onSelect(i)}
          onInclude={(included) => onInclude(i, included)}
        />
      ))}
    </div>
  );
}
