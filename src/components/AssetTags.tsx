import { useState, type FormEvent } from "react";
import { X } from "lucide-react";

import { Tag, TextInput } from "@ldlework/toybox-sdk/ui";
import type { Asset, LibraryInfo } from "../domain/catalog";
import { tagProblem } from "../domain/libraryTags";
import { useLibraryTags } from "./useLibraryTags";

/** Why tags cannot be changed at all, in plain words (no copy of the asset can be written). */
function readOnlyReason(library: LibraryInfo | undefined): string {
  if (library?.kind === "remote" && (library.offline || library.remote?.offline)) return "The server cannot be reached right now, so tags cannot be changed.";
  if (library?.kind === "remote" && library.remote?.role === "read") return "Your access token can only read, so tags cannot be changed.";
  return "This library is read-only, so tags cannot be changed.";
}

/**
 * The tags of an asset. A tag belongs to the asset and is saved in the library
 * where its file is; an asset that exists in several libraries shows the tags of
 * all its copies and a change is saved in each of them. When some library cannot
 * be written the others are still saved and the result says which was not.
 */
export function AssetTags({ asset, library }: { asset: Asset; library: LibraryInfo | undefined }) {
  const { tags, leftover, editable, change } = useLibraryTags(asset);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const run = async (work: () => Promise<{ message: string; failed: boolean }>) => {
    setBusy(true);
    setProblem(null);
    setNotice(null);
    try {
      const result = await work();
      if (result.failed) setNotice(result.message);
      return !result.failed;
    } catch (failure) {
      setProblem(String(failure instanceof Error ? failure.message : failure));
      return false;
    } finally { setBusy(false); }
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const bad = tagProblem(text);
    if (bad) { setProblem(bad); return; }
    if (await run(() => change({ add: [text] }))) setText("");
  };

  if (!editable && !tags.length && !leftover.length) return null;
  return <section className="asset-properties__tags">
    <div>Tags</div>
    {(!!tags.length || !!leftover.length) && <div className="asset-properties__tag-list" role="list" aria-label="Tags">
      {tags.map((tag) => <Tag key={tag} role="listitem" className="asset-tag">
        <span>{tag}</span>
        {editable && <button type="button" className="asset-tag__remove" aria-label={`Remove tag ${tag}`} title="Remove" disabled={busy} onClick={() => void run(() => change({ remove: [tag] }))}><X size={11} aria-hidden="true" /></button>}
      </Tag>)}
      {leftover.map((tag) => <Tag key={`kept:${tag}`} role="listitem" className="asset-tag"><span>{tag}</span></Tag>)}
    </div>}
    {editable && <form className="asset-tag-add" onSubmit={(event) => void submit(event)}>
      <TextInput value={text} onChange={(event) => { setText(event.target.value); setProblem(null); }} placeholder="Add a tag" aria-label="Add a tag" maxLength={80} disabled={busy} />
    </form>}
    {!editable && <small className="asset-tag-add__hint">{readOnlyReason(library)}</small>}
    {notice && <small className="asset-tag-add__error" role="alert">{notice}</small>}
    {problem && <small className="asset-tag-add__error" role="alert">{problem}</small>}
  </section>;
}
