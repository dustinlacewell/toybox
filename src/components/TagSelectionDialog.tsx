import { useMemo, useState } from "react";

import { Button, Dialog, TextInput } from "@ldlework/toybox-sdk/ui";
import type { Asset } from "../domain/catalog";
import type { AlsoIn } from "../domain/duplicates";
import {
  copiesOf, describeResult, parseTagInput, planIsEmpty, planTagChange, selectionTagCounts,
  type TagResult,
} from "../domain/tagEditing";
import { libraryName, runTagPlan, writeReason } from "../services/tagEdits";
import { useStore } from "../state/store";
import { useAssets } from "../state/assetCache";
import "./TagSelectionDialog.css";

/**
 * Add tags to, or remove tags from, every selected asset. The tags are saved in the
 * library each asset lives in, and in every other library that holds a copy of it
 * (one call per library, chunked); the result says per library what was saved.
 */
export function TagSelectionDialog({ assets, alsoIn, onClose }: { assets: Asset[]; alsoIn: ReadonlyMap<string, AlsoIn[]>; onClose: () => void }) {
  // The other copies of the selected items are fetched by id: the window holds no catalog.
  const copyIds = useMemo(() => [...new Set(assets.flatMap((asset) => (alsoIn.get(asset.id) ?? []).map((copy) => copy.id)))].sort(), [assets, alsoIn]);
  const copies = useAssets(copyIds.length ? copyIds : undefined).assets;
  useStore((state) => state.libraries);
  const [text, setText] = useState("");
  const [removing, setRemoving] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number }>();
  const [result, setResult] = useState<TagResult>();
  const [error, setError] = useState<string>();

  const groups = useMemo(() => {
    const byId = new Map<string, Asset>(copies.map((copy) => [copy.id, copy]));
    return assets.map((asset) => copiesOf(asset, alsoIn.get(asset.id) ?? [], (id) => byId.get(id)));
  }, [assets, alsoIn, copies]);
  const counts = useMemo(() => selectionTagCounts(groups), [groups]);
  const parsed = useMemo(() => parseTagInput(text), [text]);
  const change = useMemo(() => ({ add: parsed.tags, remove: [...removing] }), [parsed, removing]);
  const plan = useMemo(() => planTagChange(groups, change, writeReason), [groups, change]);
  const nothing = !parsed.tags.length && !removing.size;
  const unsaved = [...plan.blocked].map(([library, why]) => `${libraryName(library)}: ${why}`);

  const toggle = (tag: string) => setRemoving((current) => { const next = new Set(current); if (!next.delete(tag)) next.add(tag); return next; });
  const apply = async () => {
    setBusy(true); setError(undefined); setProgress(undefined);
    try {
      const outcomes = await runTagPlan(plan, setProgress);
      setResult(describeResult(outcomes, plan.blocked, libraryName));
    } catch (failure) { setError(String(failure instanceof Error ? failure.message : failure)); }
    finally { setBusy(false); }
  };

  const footer = result
    ? <Button variant="primary" onClick={onClose}>Close</Button>
    : <><Button disabled={busy} onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || nothing || plan.edits.size === 0} onClick={() => void apply()}>{busy ? "Saving…" : "Apply"}</Button></>;

  return <Dialog open size="compact" title={`Tag ${assets.length} ${assets.length === 1 ? "item" : "items"}`} onClose={() => { if (!busy) onClose(); }} footer={footer}>
    <div className="tag-selection">
      {result ? <p role="status" className={result.failed ? "tag-selection__error" : undefined}>{result.message}</p> : <>
        <label className="tag-selection__label">Add to all selected
          <TextInput autoFocus aria-label="Tags to add" placeholder="Tags, separated by commas" value={text} onChange={(event) => setText(event.currentTarget.value)} disabled={busy}
            onKeyDown={(event) => { if (event.key === "Enter" && !busy && !nothing && plan.edits.size) { event.preventDefault(); void apply(); } }} />
        </label>
        {parsed.problem && <small className="tag-selection__error" role="alert">{parsed.problem}</small>}
        <div className="tag-selection__label">On the selection</div>
        {counts.length ? <ul className="tag-selection__tags" aria-label="Tags on the selection">
          {counts.map(({ tag, count }) => <li key={tag} className={removing.has(tag) ? "is-removing" : undefined}>
            <span className="tag-selection__tag">{tag}</span>
            <span className="tag-selection__count">{count === assets.length ? "all" : `${count} of ${assets.length}`}</span>
            <Button variant="ghost" disabled={busy} aria-pressed={removing.has(tag)} onClick={() => toggle(tag)}>{removing.has(tag) ? "Keep" : "Remove from all"}</Button>
          </li>)}
        </ul> : <p className="tag-selection__hint">None of the selected items has a tag yet.</p>}
        {unsaved.length > 0 && <small className="tag-selection__hint" role="status">Cannot be changed: {unsaved.join("; ")}.</small>}
        {!nothing && planIsEmpty(plan) && <small className="tag-selection__hint">No item would change.</small>}
        {busy && progress && <div className="tag-selection__progress"><progress value={progress.done} max={Math.max(progress.total, 1)} /><span>{progress.done} of {progress.total}</span></div>}
        {error && <p className="tag-selection__error" role="alert">{error}</p>}
      </>}
    </div>
  </Dialog>;
}
