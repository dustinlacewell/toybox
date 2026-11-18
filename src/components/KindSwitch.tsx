/**
 * The global type switch (Models / Materials / … / any plugin-defined type).
 * Reads and writes the filter's asset type in the store; the options are the
 * registered asset types.
 */

import { useStore } from "../state/store";
import { useAssetTypes } from "../state/assetTypes";
import { Segmented } from "@ldlework/toybox-sdk/ui";

export function KindSwitch() {
  const kind = useStore((s) => s.filter.kind);
  const setKind = useStore((s) => s.setKind);
  const types = useAssetTypes((s) => s.types);
  return <Segmented value={kind} options={types.map(({ id, label }) => ({ value: id, label }))} onChange={setKind} />;
}
