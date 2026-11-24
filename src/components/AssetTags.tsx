import { useState, type MouseEvent } from "react";

import { Chip } from "@ldlework/toybox-sdk/ui";
import { assetType, type Asset, type LibraryInfo } from "../domain/catalog";
import { useLibraryTags } from "./useLibraryTags";
import { facetTitle, type OnFacet } from "./FacetLink";
import { TagAdder } from "./TagAdder";
import "./AssetTags.css";

/** Why tags cannot be changed at all, in plain words (no copy of the asset can be written). */
function readOnlyReason(library: LibraryInfo | undefined): string {
  if (library?.kind === "remote" && (library.offline || library.remote?.offline)) return "The server cannot be reached right now, so tags cannot be changed.";
  if (library?.kind === "remote" && library.remote?.role === "read") return "Your access token can only read, so tags cannot be changed.";
  return "This library is read-only, so tags cannot be changed.";
}

interface Props {
  asset: Asset;
  library: LibraryInfo | undefined;
  /** Makes each tag a filter: click requires it, Shift-click leaves it out. */
  onFacet?: OnFacet;
  /** Open the tag suggestions upward (the strip sits low in the window). */
  suggestAbove?: boolean;
}

/**
 * The tags of an asset as a strip of chips. A tag belongs to the asset and is saved in the library
 * where its file is; an asset that exists in several libraries shows the tags of
 * all its copies and a change is saved in each of them. When some library cannot
 * be written the others are still saved and the result says which was not.
 */
export function AssetTags({ asset, library, onFacet, suggestAbove }: Props) {
  const { tags, leftover, editable, change } = useLibraryTags(asset);
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
  const filter = (tag: string) => onFacet && ((event: MouseEvent) => onFacet("tags", tag, event.shiftKey));
  const title = (tag: string) => onFacet && facetTitle(tag);

  if (!editable && !tags.length && !leftover.length) return null;
  return <div className="asset-tags">
    <div className="asset-tags__list" role="list" aria-label="Tags">
      {tags.map((tag) => <Chip key={tag} role="listitem" label={tag} title={title(tag)} onPress={filter(tag)}
        onRemove={editable ? () => void run(() => change({ remove: [tag] })) : undefined} removeLabel={`Remove tag ${tag}`} disabled={busy} />)}
      {leftover.map((tag) => <Chip key={`kept:${tag}`} role="listitem" label={tag} title={title(tag)} onPress={filter(tag)} />)}
      {editable && <TagAdder kind={assetType(asset)} have={[...tags, ...leftover]} busy={busy} above={suggestAbove}
        onAdd={(tag) => run(() => change({ add: [tag] }))} onProblem={setProblem} />}
    </div>
    {!editable && <small className="asset-tags__hint">{readOnlyReason(library)}</small>}
    {notice && <small className="asset-tags__error" role="alert">{notice}</small>}
    {problem && <small className="asset-tags__error" role="alert">{problem}</small>}
  </div>;
}
