/**
 * The filter pane: the values of each field with live counts. A row is off, required (✓) or left out (−): a click
 * requires it or lets it go, "Only" makes it the one required value, "Leave out" (or Alt-click) leaves it out.
 * Tags can be matched all or any. The type's declared properties (`AssetTypeProperty`) are ranges.
 */

import { memo, useEffect, useMemo, useState, type ReactNode } from "react";
import { Bookmark, Check, ChevronDown, ChevronRight, Minus, Star, X } from "lucide-react";

import type { LibraryInfo } from "../domain/catalog";
import { FIND_FROM, facetState, formatLabel, narrowRows, type ExcludableField, type FacetField, type Filter } from "../domain/facets";
import type { VisibilityRules } from "../domain/browseQuery";
import type { Count, FacetCounts } from "../services/browse";
import type { AssetTypeProperty } from "../services/assetTypes";
import { useTagSearch } from "../state/libraryCounts";
import { Button, TextInput } from "@ldlework/toybox-sdk/ui";
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
  /** Range properties of the type shown. */
  properties: AssetTypeProperty[];
  onFacet: (field: FacetField, value: string, state: "in" | "out" | "off", only?: boolean) => void;
  onTagsMatch: (match: Filter["tagsMatch"]) => void;
  onRange: (path: string, bounds: { min?: number; max?: number } | null) => void;
  onFavorites: () => void;
  onClear: () => void;
  onSaveView?: () => void;
  onClose?: () => void;
}

const byValue = (counts: Count[] | undefined): Count[] => [...(counts ?? [])].sort((a, b) => a.value.localeCompare(b.value));

/** The rows of a field: the counted values plus any chosen or left-out value the counts do not list. */
function rowsOf(counts: Count[] | undefined, chosen: ReadonlySet<string>, excluded?: ReadonlySet<string>): Count[] {
  const rows = byValue(counts);
  for (const value of [...chosen, ...(excluded ?? [])]) if (!rows.some((row) => row.value === value)) rows.push({ value, count: 0 });
  return rows;
}

export const FacetFilter = memo(function FacetFilter({ counts, rules, filter, libraries = [], properties, onFacet, onTagsMatch, onRange, onFavorites, onClear, onSaveView, onClose }: Props) {
  const inLibrary = useMemo(() => new Map((counts?.libraries ?? []).map(({ value, count }) => [value, count])), [counts]);
  const [tagSearch, setTagSearch] = useState("");
  const [allTags, setAllTags] = useState(false);
  const found = useTagSearch(filter.kind, rules, tagSearch);
  const tagTotal = counts?.tagTotal ?? 0;
  const tagRows = useMemo(() => {
    const rows = found ? byValue(found) : rowsOf(counts?.tags, filter.tags, filter.exclude.tags);
    return { rows, hidden: found ? 0 : Math.max(tagTotal - rows.length, 0) };
  }, [counts, found, tagTotal, filter.tags, filter.exclude.tags]);
  // Like the other fields: the first few until "Show more"; a search shows every match.
  const shownTags = narrowRows(tagRows.rows, "", (value) => facetState(filter, "tags", value) !== "off", (count) => count.value, FIND_FROM, allTags || !!found);
  const row = (field: FacetField, value: string, count: number, label: ReactNode = value || "Unknown", mark?: ReactNode) =>
    <FacetRow key={value} label={label} mark={mark} count={count} state={facetState(filter, field, value)} canExclude={field !== "libraries"} onChange={(state, only) => onFacet(field, value, state, only)} />;

  return (
    <aside className="facets" aria-label="Filters">
      <div className="facets__head">
        <h2>Filters</h2>
        <button type="button" className="facets__link" onClick={onClear}>Clear all</button>
        {onClose && <button type="button" className="facets__close" aria-label="Close filters" onClick={onClose}><X size={16} /></button>}
      </div>
      <div className="facets__scroll">
        <button type="button" className="facets__favorites" aria-pressed={filter.favoritesOnly} onClick={onFavorites}><Star size={16} aria-hidden="true" />Favorites only</button>

        {libraries.length > 1 && <FacetGroup title="Libraries">{libraries.map((library) => row("libraries", library.id, inLibrary.get(library.id) ?? 0, library.name, <RemoteMark library={library} />))}</FacetGroup>}
        <FieldGroup title="Publishers" noun="publishers" field="publishers" filter={filter} rows={rowsOf(counts?.publishers, filter.publishers, filter.exclude.publishers)} row={row} />
        {(tagTotal > 0 || filter.tags.size > 0 || filter.exclude.tags.size > 0) && <FacetGroup title="Tags" mode={filter.tags.size > 1 ? { value: filter.tagsMatch, onChange: onTagsMatch } : undefined}>
          {tagTotal > FIND_FROM && <TextInput className="facets__tag-search" aria-label="Find a tag" placeholder={`Find in ${tagTotal.toLocaleString()} tags`} value={tagSearch} onChange={(event) => setTagSearch(event.currentTarget.value)} />}
          {shownTags.rows.map(({ value, count }) => row("tags", value, count))}
          {shownTags.hidden > 0 && <button type="button" className="facets__more" onClick={() => setAllTags(true)}>Show {shownTags.hidden.toLocaleString()} more</button>}
          {allTags && !found && tagRows.rows.length > FIND_FROM && <button type="button" className="facets__more" onClick={() => setAllTags(false)}>Show fewer</button>}
          {tagRows.hidden > 0 && (allTags || !!found) && <span className="facets__empty">{tagRows.hidden.toLocaleString()} more: search to find them</span>}
          {!tagRows.rows.length && <span className="facets__empty">No tag matches</span>}
        </FacetGroup>}
        <FieldGroup title="Sources" noun="sources" field="sources" filter={filter} rows={rowsOf(counts?.sources, filter.sources, filter.exclude.sources)} row={row} />
        <FieldGroup title="Licences" noun="licences" field="licenses" filter={filter} rows={rowsOf(counts?.licenses, filter.licenses, filter.exclude.licenses)} row={row} />
        {(counts?.formats.length ?? 0) > 1 && <FieldGroup title="Formats" noun="formats" field="formats" filter={filter} rows={rowsOf(counts?.formats, filter.formats, filter.exclude.formats)} row={row} label={formatLabel} />}
        <FieldGroup title="Category" noun="categories" field="categories" filter={filter} rows={rowsOf(counts?.categories, filter.categories, filter.exclude.categories)} row={row} label={titleCase} />
        {properties.length > 0 && <FacetGroup title="Properties">
          {properties.map((property) => <RangeRow key={property.path} property={property} bounds={filter.ranges[property.path]} onChange={(bounds) => onRange(property.path, bounds)} />)}
        </FacetGroup>}
      </div>
      {onSaveView && <div className="facets__foot"><Button onClick={onSaveView}><Bookmark size={14} aria-hidden="true" />Save as view</Button></div>}
    </aside>
  );
});

/** A field's rows; with more than `FIND_FROM` values, a find field and "Show more". Chosen and left-out values stay in view. */
function FieldGroup({ title, noun, field, filter, rows, row, label = (value) => value || "Unknown" }: {
  title: string; noun: string; field: ExcludableField; filter: Filter; rows: Count[];
  row: (field: FacetField, value: string, count: number, label?: ReactNode) => ReactNode;
  label?: (value: string) => string;
}) {
  const [needle, setNeedle] = useState("");
  const [all, setAll] = useState(false);
  const long = rows.length > FIND_FROM;
  const keep = (value: string) => facetState(filter, field, value) !== "off";
  const shown = narrowRows(rows, long ? needle : "", keep, (count) => label(count.value), FIND_FROM, all || !!needle.trim());
  return <FacetGroup title={title}>
    {long && <TextInput className="facets__tag-search" aria-label={`Find a ${noun.replace(/s$/, "")}`} placeholder={`Find in ${rows.length.toLocaleString()} ${noun}`} value={needle} onChange={(event) => setNeedle(event.currentTarget.value)} />}
    {shown.rows.map(({ value, count }) => row(field, value, count, label(value)))}
    {needle.trim() && !shown.rows.some((count) => !keep(count.value)) && <span className="facets__empty">No {noun} contain “{needle.trim()}”</span>}
    {shown.hidden > 0 && <button type="button" className="facets__more" onClick={() => setAll(true)}>Show {shown.hidden.toLocaleString()} more</button>}
    {all && long && !needle.trim() && <button type="button" className="facets__more" onClick={() => setAll(false)}>Show fewer</button>}
  </FacetGroup>;
}

function FacetGroup({ title, mode, children }: { title: string; mode?: { value: Filter["tagsMatch"]; onChange: (value: Filter["tagsMatch"]) => void }; children: ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div className="facets__section" role="group" aria-label={title}>
      <div className="facets__title">
        <button type="button" className="facets__toggle" aria-expanded={open} onClick={() => setOpen(!open)}>{open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}<span>{title}</span></button>
        {mode && <span className="facets__mode" role="radiogroup" aria-label="Match">
          {(["all", "any"] as const).map((value) => <button type="button" key={value} role="radio" aria-checked={mode.value === value} onClick={() => mode.onChange(value)}>{value === "all" ? "All" : "Any"}</button>)}
        </span>}
      </div>
      {open && children}
    </div>
  );
}

function FacetRow({ label, mark, count, state, canExclude, onChange }: {
  label: ReactNode;
  /** Shown after the label (the cloud of a remote library). */
  mark?: ReactNode;
  count: number;
  state: "in" | "out" | "off";
  canExclude: boolean;
  onChange: (state: "in" | "out" | "off", only?: boolean) => void;
}) {
  return (
    <div className={`facets__row facets__row--${state}`}>
      <button type="button" className="facets__main" aria-pressed={state === "in"} title={canExclude ? "Click to require · Alt-click to leave out" : undefined}
        onClick={(event) => onChange(canExclude && event.altKey ? (state === "out" ? "off" : "out") : state === "in" ? "off" : "in")}>
        <span className="facets__box">{state === "in" ? <Check size={12} /> : state === "out" ? <Minus size={12} /> : null}</span>
        <span className="facets__label">{label}{mark}</span>
        <span className="facets__count">{count.toLocaleString()}</span>
      </button>
      <span className="facets__acts">
        <button type="button" onClick={() => onChange("in", true)}>Only</button>
        {canExclude && <button type="button" className="facets__out" onClick={() => onChange(state === "out" ? "off" : "out")}>{state === "out" ? "Keep" : "Leave out"}</button>}
      </span>
    </div>
  );
}

/** Two number fields, applied when one is left (or Enter). */
function RangeRow({ property, bounds, onChange }: { property: AssetTypeProperty; bounds?: { min?: number; max?: number }; onChange: (bounds: { min?: number; max?: number } | null) => void }) {
  const [min, setMin] = useState(bounds?.min?.toString() ?? "");
  const [max, setMax] = useState(bounds?.max?.toString() ?? "");
  useEffect(() => { setMin(bounds?.min?.toString() ?? ""); setMax(bounds?.max?.toString() ?? ""); }, [bounds?.min, bounds?.max]);
  const number = (text: string) => (text.trim() && Number.isFinite(Number(text)) ? Number(text) : undefined);
  const apply = () => {
    const next = { min: number(min), max: number(max) };
    onChange(next.min === undefined && next.max === undefined ? null : next);
  };
  const unit = property.unit ? ` (${property.unit})` : "";
  return <div className="facets__range">
    <span className="facets__range-label">{property.label}{unit}</span>
    <span className="facets__range-fields">
      <TextInput inputMode="decimal" aria-label={`${property.label} at least`} placeholder="min" value={min} onChange={(event) => setMin(event.currentTarget.value)} onBlur={apply} onKeyDown={(event) => { if (event.key === "Enter") apply(); }} />
      <span>–</span>
      <TextInput inputMode="decimal" aria-label={`${property.label} at most`} placeholder="max" value={max} onChange={(event) => setMax(event.currentTarget.value)} onBlur={apply} onKeyDown={(event) => { if (event.key === "Enter") apply(); }} />
    </span>
  </div>;
}

const titleCase = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
