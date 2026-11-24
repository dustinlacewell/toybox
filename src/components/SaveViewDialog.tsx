import { useState } from "react";
import { Button, Dialog, TextInput } from "@ldlework/toybox-sdk/ui";

/** Name the current filter, sort and type as a saved view. A name already in use replaces that view. */
export function SaveViewDialog({ names, suggestion, onClose, onSave }: { names: string[]; suggestion: string; onClose: () => void; onSave: (name: string) => void }) {
  const [name, setName] = useState(suggestion);
  const taken = names.includes(name.trim());
  const save = () => { if (name.trim()) { onSave(name.trim()); onClose(); } };
  return <Dialog open size="compact" title="Save view" onClose={onClose} footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!name.trim()} onClick={save}>{taken ? "Replace" : "Save"}</Button></>}>
    <div className="collection-dialog">
      <p>The view keeps the search, the filters, the order and the type, and shows what matches whenever it is opened.</p>
      <TextInput autoFocus aria-label="View name" placeholder="View name" value={name} onChange={(event) => setName(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === "Enter") save(); }} />
      {taken && <p style={{ color: "var(--text-2)" }}>A view of this name exists; saving replaces it.</p>}
    </div>
  </Dialog>;
}
