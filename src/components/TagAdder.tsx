import { useState, type KeyboardEvent } from "react";
import { Plus } from "lucide-react";
import { SuggestionList, TextInput, useActiveRow } from "@ldlework/toybox-sdk/ui";

import { tagSuggestions } from "../domain/assetCard";
import { tagProblem } from "../domain/libraryTags";
import { useTagSearch } from "../state/libraryCounts";
import { usePlugins } from "../state/plugins";

interface Props {
  /** The asset type whose tags are suggested. */
  kind: string;
  /** Tags the item has: never suggested. */
  have: readonly string[];
  busy: boolean;
  /** Open the suggestions over the field (when it sits low in the window). */
  above?: boolean;
  /** Add a tag; resolves whether it was saved. */
  onAdd: (tag: string) => Promise<boolean>;
  onProblem: (problem: string | null) => void;
}

/**
 * "+ tag": opens a field that suggests the type's tags as it is typed in. ArrowUp/ArrowDown choose a suggestion;
 * Enter adds it, or the typed text when none is chosen; Escape closes the field.
 */
export function TagAdder({ kind, have, busy, above = false, onAdd, onProblem }: Props) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const rules = usePlugins((state) => state.rules);
  const rows = tagSuggestions(useTagSearch(kind, rules, text), have);
  const { active, setActive, arrows } = useActiveRow(rows.length, text, -1);

  const close = () => { setOpen(false); setText(""); };
  const add = async (tag: string) => {
    const bad = tagProblem(tag);
    if (bad) { onProblem(bad); return; }
    if (await onAdd(tag)) setText("");
  };
  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    if (rows.length && arrows(event)) return;
    if (event.key === "Enter") { event.preventDefault(); void add(rows[active]?.value ?? text); }
    else if (event.key === "Escape") { event.preventDefault(); close(); }
  };

  if (!open) return <button type="button" className="tag-adder__open" onClick={() => setOpen(true)}><Plus size={12} aria-hidden="true" />tag</button>;
  return <span className="tag-adder">
    <TextInput className="tag-adder__input" value={text} autoFocus readOnly={busy} maxLength={80} placeholder="Add a tag" aria-label="Add a tag"
      onChange={(event) => { setText(event.target.value); onProblem(null); }} onKeyDown={keys} onBlur={close} />
    {!!text.trim() && rows.length > 0 && <SuggestionList className="tag-adder__suggestions" placement={above ? "above" : "below"} label="Tags"
      rows={rows.map((row) => ({ key: row.value, label: row.value, count: row.count }))} active={active}
      onHover={setActive} onPick={(index) => void add(rows[index].value)} />}
  </span>;
}
