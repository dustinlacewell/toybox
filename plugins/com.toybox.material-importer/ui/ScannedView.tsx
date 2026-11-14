/**
 * The drawer panel after a scan: how many materials were found, the list of them
 * (picture and assigned roles each, include checkboxes, double-click to review),
 * and the buttons to review, rescan or import.
 */

import { useMemo } from "react";
import { Button, Stack } from "@ldlework/toybox-sdk/ui";
import { canImport, importable, problemsFor, type ReviewItem } from "./edit.js";
import { MaterialList } from "./MaterialList.js";
import type { ThumbLoader } from "./thumbs.js";

interface Props {
  items: ReviewItem[];
  /** Name of the scanned folder. */
  folder: string;
  pack: string;
  category: string;
  /** Undefined until the loader for this folder exists. */
  loader: ThumbLoader | undefined;
  /** Open the review dialog on this item. */
  onOpen: (index: number) => void;
  onInclude: (index: number, included: boolean) => void;
  onScan: () => void;
  onImport: () => void;
}

export function ScannedView({
  items,
  folder,
  pack,
  category,
  loader,
  onOpen,
  onInclude,
  onScan,
  onImport,
}: Props) {
  const n = items.length;
  const problems = useMemo(
    () => items.map((_, i) => problemsFor(items, i, pack, category)),
    [items, pack, category],
  );
  const indexes = useMemo(() => items.map((_, i) => i), [items]);
  const count = useMemo(() => importable(items, pack, category).length, [items, pack, category]);

  return (
    <section className="export__section">
      <div className="export__hint">
        {n} material{n === 1 ? "" : "s"} found in {folder}
      </div>
      {loader && (
        <MaterialList
          items={items}
          problems={problems}
          indexes={indexes}
          maxHeight="50vh"
          loader={loader}
          onOpen={onOpen}
          onInclude={onInclude}
        />
      )}
      <Stack dir="row" gap={8}>
        <Button onClick={() => onOpen(0)}>Review…</Button>
        <Button onClick={onScan}>Choose a different folder…</Button>
      </Stack>
      <Button variant="primary" disabled={!canImport(items, pack, category)} onClick={onImport}>
        Import {count} into library
      </Button>
    </section>
  );
}
