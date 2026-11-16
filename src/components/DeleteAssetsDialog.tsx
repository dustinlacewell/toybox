import { useState } from "react";
import { ConfirmationDialog } from "@ldlework/toybox-sdk/ui";
import { File } from "lucide-react";
import type { Asset } from "../domain/catalog";
import { useThumbUrl } from "./useThumbUrl";

function ItemPreview({ asset }: { asset: Asset }) {
  const url = useThumbUrl(asset);
  return url ? <img src={url} alt="" /> : <File size={18} />;
}

export function DeleteAssetsDialog({ assets, onClose, onDelete }: {
  assets: Asset[]; onClose: () => void; onDelete: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const label = `${assets.length} ${assets.length === 1 ? "item" : "items"}`;
  return <ConfirmationDialog title={`Delete ${label}?`} onClose={onClose}
    description="Delete these items and their files, including all variants?"
    items={assets.map(asset => ({ id: asset.id, label: asset.name, detail: asset.pack.replace(/_/g, " "), preview: <ItemPreview asset={asset} /> }))}
    note="This cannot be undone. Files shared with other library items will be kept."
    confirmLabel={`Delete ${label}`} busyLabel="Deleting…" busy={busy} disabled={!assets.length} error={error} onConfirm={async () => {
      setBusy(true); setError(undefined);
      try { await onDelete(); onClose(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
    }} />;
}
