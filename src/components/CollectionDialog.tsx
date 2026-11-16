import { useState } from "react";
import { Button, Dialog, TextInput } from "@ldlework/toybox-sdk/ui";

export function CollectionDialog({ names, count, onClose, onSave }: { names: string[]; count: number; onClose: () => void; onSave: (name: string) => Promise<void> }) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  return <Dialog open size="compact" title="Add to collection" onClose={() => { if (!busy) onClose(); }} footer={<><Button disabled={busy} onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!name.trim() || busy} onClick={async () => {
    setBusy(true); setError(undefined);
    try { await onSave(name.trim()); onClose(); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }}>{busy ? "Adding…" : `Add ${count}`}</Button></>}>
    <div className="collection-dialog"><p>Choose a collection or name a new one.</p><TextInput autoFocus aria-label="Collection name" placeholder="Collection name" value={name} onChange={(e) => setName(e.currentTarget.value)} disabled={busy} />
      {names.map((n) => <Button key={n} variant="ghost" onClick={() => setName(n)} disabled={busy}>{n}</Button>)}{error && <p role="alert">{error}</p>}
    </div>
  </Dialog>;
}
