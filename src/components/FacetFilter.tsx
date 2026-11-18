/**
 * Metadata filters with live counts, supplied to controlled design-system inputs.
 */

import { memo, useMemo, useState } from "react";
import type { Category, LibraryInfo } from "../domain/catalog";
import { formatLabel, type Filter } from "../domain/facets";
import type { VisibilityRules } from "../domain/browseQuery";
import type { Count, FacetCounts } from "../services/browse";
import { TAG_ROWS, useTagSearch } from "../state/libraryCounts";
import { Button, IconButton, TextInput } from "@ldlework/toybox-sdk/ui";
import { Star, FilterX } from "lucide-react";
import { RemoteMark } from "./RemoteMark";
import "./FacetFilter.css";

interface Props {
  /** The numbers to show, asked of the libraries (see `useFacetCounts`); nothing until they arrive. */
  counts?: FacetCounts;
  /** The hidden-plugin rules the numbers were made with (the tag search asks with them). */
  rules: VisibilityRules;
  filter: Filter;
  /** The registered libraries; the Libraries group shows only with more than one. */
  libraries?: LibraryInfo[];
  onToggleLibrary?: (library: string, additive: boolean) => void;
  onToggleSource: (source: string, additive: boolean) => void;
  onTogglePublisher: (publisher: string, additive: boolean) => void;
  onToggleLicense: (license: string, additive: boolean) => void;
  onToggleFormat: (format: string, additive: boolean) => void;
  onToggleCategory: (category: Category, additive: boolean) => void;
  onToggleTag?: (tag:string, additive: boolean) => void;
  onToggleFavoritesOnly: () => void;
  onClear: () => void;
}

/** The filter fields the rail shows; the search text is not one of them. */
const FILTER_SETS = ["libraries", "sources", "publishers", "licenses", "formats", "categories", "tags"] as const;

/** Typing in the search box changes only `filter.text`, which the rail does not show, so it must not
 *  re-render the rail. Handlers are left out: they only call store setters. */
function sameRail(a: Props, b: Props): boolean {
  return a.counts === b.counts && a.rules === b.rules && a.libraries === b.libraries && a.filter.kind === b.filter.kind &&
    a.filter.favoritesOnly === b.filter.favoritesOnly && FILTER_SETS.every(key => a.filter[key] === b.filter[key]) &&
    !a.onToggleLibrary === !b.onToggleLibrary && !a.onToggleTag === !b.onToggleTag;
}

const byValue = (counts: Count[] | undefined): Count[] => [...(counts ?? [])].sort((a, b) => a.value.localeCompare(b.value));

export const FacetFilter = memo(function FacetFilter({
  counts,
  rules,
  filter,
  libraries = [],
  onToggleLibrary,
  onToggleSource,
  onTogglePublisher,
  onToggleLicense,
  onToggleFormat,
  onToggleCategory,
  onToggleTag,
  onToggleFavoritesOnly,
  onClear,
}: Props) {
  // Counts cover only the current type, each item once, and are asked for once per change of the libraries.
  const categories = useMemo(() => byValue(counts?.categories), [counts]);
  const formats = useMemo(() => byValue(counts?.formats), [counts]);
  const inLibrary = useMemo(() => new Map((counts?.libraries ?? []).map(({ value, count }) => [value, count])), [counts]);
  const provenance = useMemo(() => ({ source: byValue(counts?.sources), publisher: byValue(counts?.publishers), license: byValue(counts?.licenses) }), [counts]);
  const [tagSearch, setTagSearch] = useState("");
  const found = useTagSearch(filter.kind, rules, tagSearch);
  const tagTotal = counts?.tagTotal ?? 0;
  // The rows shown: the most used and the chosen ones; searching asks the libraries for the tags that match, so every tag can be found.
  const tagRows = useMemo(() => {
    const rows = found ? byValue(found) : byValue(counts?.tags);
    return { rows, hidden: found ? 0 : Math.max(tagTotal - rows.length, 0) };
  }, [counts, found, tagTotal]);

  return (
    <aside className="facets">
      <div className="facets__toolbar">
        <Button variant="ghost" className="facets__row facets__favorites" aria-pressed={filter.favoritesOnly} onClick={onToggleFavoritesOnly}><Star size={16} aria-hidden="true"/>Favorites</Button>
        <IconButton icon={FilterX} label="Clear filters" onClick={onClear}/>
      </div>

      {onToggleLibrary&&libraries.length>1&&<FacetGroup title="Libraries">{libraries.map((library)=>{const {id,name}=library;return <FacetRow key={id} label={name} mark={<RemoteMark library={library}/>} count={inLibrary.get(id)??0} checked={filter.libraries.has(id)} onToggle={additive=>onToggleLibrary(id,additive)}/>;})}</FacetGroup>}
      {([{field:"source",title:"Sources",selected:filter.sources,onToggle:onToggleSource},
        {field:"publisher",title:"Publishers",selected:filter.publishers,onToggle:onTogglePublisher},
        {field:"license",title:"Licenses",selected:filter.licenses,onToggle:onToggleLicense}] as const).map(group=><FacetGroup key={group.field} title={group.title}>
          {provenance[group.field].map(({value,count})=><FacetRow key={value} label={value||"Unknown"} count={count} checked={group.selected.has(value)} onToggle={additive=>group.onToggle(value,additive)}/>)}
      </FacetGroup>)}
      {formats.length>1&&<FacetGroup title="Formats">{formats.map(({value,count})=><FacetRow key={value} label={formatLabel(value)} count={count} checked={filter.formats.has(value)} onToggle={additive=>onToggleFormat(value,additive)}/>)}</FacetGroup>}
      {onToggleTag&&tagTotal>0&&<FacetGroup title="Tags">
        {tagTotal>TAG_ROWS&&<TextInput className="facets__tag-search" aria-label="Find a tag" placeholder={`Find in ${tagTotal.toLocaleString()} tags`} value={tagSearch} onChange={event=>setTagSearch(event.currentTarget.value)}/>}
        {tagRows.rows.map(({value,count})=><FacetRow key={value} label={value} count={count} checked={filter.tags.has(value)} onToggle={additive=>onToggleTag(value,additive)}/>)}
        {tagRows.hidden>0&&<span className="facets__empty">{tagRows.hidden.toLocaleString()} more: search to find them</span>}
        {!tagRows.rows.length&&<span className="facets__empty">No tag matches</span>}
      </FacetGroup>}

      <FacetGroup title="Category">
        {categories.map(({ value, count }) => (
          <FacetRow
            key={value}
            label={titleCase(value)}
            count={count}
            checked={filter.categories.has(value)}
            onToggle={additive => onToggleCategory(value,additive)}
          />
        ))}
      </FacetGroup>

    </aside>
  );
}, sameRail);

function FacetGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="facets__section" role="group" aria-label={title}>
      <div className="facets__title" title="Click to select; Shift-click to add or remove">{title}</div>
      {children}
    </div>
  );
}

function FacetRow({
  label,
  mark,
  count,
  checked,
  onToggle,
}: {
  label: string;
  /** Shown after the label (the cloud of a remote library). */
  mark?: React.ReactNode;
  count: number;
  checked: boolean;
  onToggle: (additive: boolean) => void;
}) {
  return (
    <Button variant="ghost" className="facets__row" aria-pressed={checked} onClick={event=>onToggle(event.shiftKey)} onMouseDown={event=>{if(event.shiftKey)event.preventDefault();}}>
      <span className="facets__label" title={label}>{label}{mark}</span>
      <span className="facets__count">{count}</span>
    </Button>
  );
}

const titleCase = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
