/**
 * The global Models / Materials switch. Reads and writes the filter's asset
 * kind in the store. The label lookup is keyed by AssetKind, so the compiler
 * rejects a kind that has no label.
 */

import type { AssetKind } from "../domain/catalog";
import { useStore } from "../state/store";
import { Segmented } from "@ldlework/toybox-sdk/ui";

const KIND_LABELS: { [K in AssetKind]: string } = {
  model: "Models",
  material: "Materials",
  animation: "Animations",
  audio: "Audio",
};

const OPTIONS = (Object.keys(KIND_LABELS) as AssetKind[]).map((value) => ({
  value,
  label: KIND_LABELS[value],
}));

export function KindSwitch() {
  const kind = useStore((s) => s.filter.kind);
  const setKind = useStore((s) => s.setKind);
  return <Segmented value={kind} options={OPTIONS} onChange={setKind} />;
}
