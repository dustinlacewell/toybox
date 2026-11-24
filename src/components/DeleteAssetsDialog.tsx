import { useMemo, useState } from "react";
import { ConfirmationDialog } from "@ldlework/toybox-sdk/ui";
import { File } from "lucide-react";
import type { Asset, LibraryInfo } from "../domain/catalog";
import { librariesHolding, type AlsoIn } from "../domain/duplicates";
import { countOf, useDerivedAssets } from "../state/derived";
import { useThumbUrl } from "./useThumbUrl";

function ItemPreview({ asset }: { asset: Asset }) {
  const url = useThumbUrl(asset);
  return url ? <img src={url} alt="" /> : <File size={18} />;
}

/** One short line when an item also lives in another library: only this copy goes. */
function copiesNote(assets: Asset[], libraries: LibraryInfo[], alsoIn: ReadonlyMap<string, AlsoIn[]>): string {
  const name = (id: string) => libraries.find((library) => library.id === id)?.name ?? id;
  const from = new Set<string>(), kept = new Set<string>();
  for (const asset of assets) {
    const others = librariesHolding(asset, alsoIn).slice(1);
    if (!others.length) continue;
    from.add(asset.library);
    for (const id of others) kept.add(id);
  }
  for (const id of from) kept.delete(id);
  if (!from.size || !kept.size) return "";
  return `Only deletes from ${[...from].map(name).join(", ")}; the copy in ${[...kept].map(name).join(", ")} stays.`;
}

/** Items made from a deleted item stay (the link is soft). Looked up for a small selection only. */
const DERIVED_CHECKED = 20;

/** At most this many items are listed in the dialog (a selection can be hundreds of thousands). */
const LISTED = 200;

export function DeleteAssetsDialog({ assets, count = assets.length, libraries = [], alsoIn = new Map(), onClose, onDelete }: {
  /** The items to list (the first of the selection). */
  assets: Asset[]; /** How many items are selected in all. */ count?: number; libraries?: LibraryInfo[]; alsoIn?: ReadonlyMap<string, AlsoIn[]>; onClose: () => void; onDelete: () => Promise<void>;
}) {
  const copies = copiesNote(assets, libraries, alsoIn);
  const checked = useMemo(() => (count <= DERIVED_CHECKED ? assets.map((asset) => asset.id) : []), [assets, count]);
  const deleted = useMemo(() => new Set(checked), [checked]);
  const kept = (useDerivedAssets(checked) ?? []).filter((asset) => !deleted.has(asset.id));
  const derived = kept.length ? `${countOf(kept)} ${kept.length === 1 ? "was" : "were"} made from ${count === 1 ? "it" : "them"}; ${kept.length === 1 ? "it stays" : "they stay"}.` : "";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const label = `${count} ${count === 1 ? "item" : "items"}`;
  const listed = assets.slice(0, LISTED);
  return <ConfirmationDialog title={`Delete ${label}?`} onClose={onClose}
    description="Delete these items and their files, including all variants?"
    items={listed.map(asset => ({ id: asset.id, label: asset.name, detail: asset.pack.replace(/_/g, " "), preview: <ItemPreview asset={asset} /> }))}
    note={`${copies ? copies + " " : ""}${derived ? derived + " " : ""}This cannot be undone. Files shared with other library items will be kept.`}
    confirmLabel={`Delete ${label}`} busyLabel="Deleting…" busy={busy} disabled={!count} error={error} onConfirm={async () => {
      setBusy(true); setError(undefined);
      try { await onDelete(); onClose(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
    }} />;
}
