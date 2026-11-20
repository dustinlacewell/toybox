/**
 * The query bar: every condition of the filter as a chip, and the search text. Typing searches as it goes; a
 * typed field term (`tag:sci-fi`, `-publisher:"Kitbash Works"`) becomes a chip on Enter. While typing, the values
 * of the fields that contain the text are suggested: Enter or a click chooses one; Shift, or a `-` before
 * the word, leaves it out.
 */

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Search, SlidersHorizontal, Tag as TagIcon, X } from "lucide-react";
import { SuggestionList, useActiveRow, type SuggestionRow } from "@ldlework/toybox-sdk/ui";

import { EXCLUDABLE, formatLabel, type ExcludableField, type Filter } from "../domain/facets";
import { filterChips, parseQuery, typingTerm, type FilterChip } from "../domain/query";
import type { VisibilityRules } from "../domain/browseQuery";
import type { Count, FacetCounts } from "../services/browse";
import { useTagSearch } from "../state/libraryCounts";
import type { AssetTypeProperty } from "../services/assetTypes";
import "./QueryBar.css";

interface Props {
  filter: Filter;
  counts?: FacetCounts;
  rules: VisibilityRules;
  properties: AssetTypeProperty[];
  placeholder: string;
  filtersOpen: boolean;
  onToggleFilters: () => void;
  onText: (text: string) => void;
  /** Typed text with field terms in it was confirmed. */
  onQuery: (text: string) => void;
  onFacet: (field: ExcludableField, value: string, state: "in" | "out" | "off") => void;
  onRange: (path: string) => void;
  onFavorites: () => void;
}

interface Suggestion { field: ExcludableField; value: string; count: number }

/** Suggestions per field shown at most. */
const PER_FIELD = 5;
const FIELD_TITLES: Record<ExcludableField, string> = { tags: "Tags", publishers: "Publishers", sources: "Sources", licenses: "Licences", formats: "Formats", categories: "Categories" };

export function QueryBar({ filter, counts, rules, properties, placeholder, filtersOpen, onToggleFilters, onText, onQuery, onFacet, onRange, onFavorites }: Props) {
  const [draft, setDraft] = useState(filter.text);
  const [focused, setFocused] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  // The filter's text changes elsewhere too (a cleared filter, a saved view).
  useEffect(() => { if (parseQuery(draft).text !== filter.text) setDraft(filter.text); }, [filter.text]); // eslint-disable-line react-hooks/exhaustive-deps

  // The word being typed: `field:part` narrows the suggestions to that field; `-field:part` leaves the pick out.
  const typing = typingTerm(draft);
  const scopeField = typing.field;
  const needle = (scopeField ? typing.part : parseQuery(draft).text).replace(/"/g, "").trim().toLowerCase();
  const foundTags = useTagSearch(filter.kind, rules, needle.length >= 1 && (!scopeField || scopeField === "tags") ? needle : "");

  const suggestions = useMemo((): Suggestion[] => {
    if (!needle) return [];
    const from = (field: ExcludableField, rows: Count[] | undefined): Suggestion[] =>
      (rows ?? []).filter((row) => row.value && row.value.toLowerCase().includes(needle) && !filter[field].has(row.value) && !filter.exclude[field].has(row.value))
        .sort((a, b) => b.count - a.count).slice(0, PER_FIELD).map((row) => ({ field, value: row.value, count: row.count }));
    const rows: Record<ExcludableField, Count[] | undefined> = {
      tags: foundTags, publishers: counts?.publishers, sources: counts?.sources,
      licenses: counts?.licenses, formats: counts?.formats, categories: counts?.categories,
    };
    // In the order the popup shows the fields, so the arrow keys walk the rows top to bottom.
    return EXCLUDABLE.filter((field) => !scopeField || field === scopeField).flatMap((field) => from(field, rows[field]));
  }, [needle, scopeField, foundTags, counts, filter]);
  const open = focused && !!needle;
  // Row 0 is "search for the text" (absent when the word is a field term).
  const offset = scopeField ? 0 : 1;
  const { active, setActive, arrows } = useActiveRow(suggestions.length + offset, needle);

  const chips = filterChips(filter, properties);
  const typed = (text: string) => { setDraft(text); onText(parseQuery(text).text); };
  /** Take the word being typed out of the draft and choose a suggestion instead; left out when the word began with
   *  `-` or Shift is held. */
  const pick = (suggestion: Suggestion, shift: boolean) => {
    const rest = draft.split(/\s+/).slice(0, -1).join(" ");
    onFacet(suggestion.field, suggestion.value, typing.exclude || shift ? "out" : "in");
    typed(rest);
    input.current?.focus();
  };
  const confirm = () => { onQuery(draft); setDraft(parseQuery(draft).text); };
  const removeChip = (chip: FilterChip) => {
    if (chip.kind === "favorites") onFavorites();
    else if (chip.kind === "range") onRange(chip.path!);
    else onFacet(chip.field!, chip.value!, "off");
  };
  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    if (open && arrows(event)) return;
    if (event.key === "Enter") {
      event.preventDefault();
      const chosen = open ? suggestions[active - offset] : undefined;
      if (chosen) pick(chosen, event.shiftKey); else confirm();
    } else if (event.key === "Escape") { setFocused(false); input.current?.blur(); }
    else if (event.key === "Backspace" && !draft && chips.length) removeChip(chips[chips.length - 1]);
  };

  return <div className="searchrow">
    <div className={`query-bar${focused ? " query-bar--focus" : ""}`} onMouseDown={(event) => { if (event.target === event.currentTarget) { event.preventDefault(); input.current?.focus(); } }}>
      <Search size={16} aria-hidden="true" className="query-bar__icon" />
      {chips.map((chip) => <span key={`${chip.kind}:${chip.field ?? chip.path ?? ""}:${chip.value ?? ""}:${chip.exclude}`} className={`query-chip${chip.exclude ? " query-chip--out" : chip.kind === "facet" ? " query-chip--in" : ""}`}>
        <button type="button" className="query-chip__body" disabled={chip.kind !== "facet"}
          title={chip.kind === "facet" ? (chip.exclude ? "Left out: click to require it instead" : "Required: click to leave it out instead") : undefined}
          onClick={() => chip.kind === "facet" && onFacet(chip.field!, chip.value!, chip.exclude ? "in" : "out")}>
          <span className="query-chip__field">{chip.label}</span>{chip.exclude && <span className="query-chip__op">not</span>}<span className="query-chip__value">{chip.field === "formats" ? formatLabel(chip.value ?? "") : chip.text}</span>
        </button>
        <button type="button" className="query-chip__x" aria-label={`Remove ${chip.label} ${chip.text}`} onClick={() => removeChip(chip)}><X size={12} /></button>
      </span>)}
      <input ref={input} className="query-bar__input" value={draft} aria-label="Search" placeholder={chips.length ? "" : placeholder}
        onChange={(event) => typed(event.currentTarget.value)} onKeyDown={keys}
        onFocus={() => setFocused(true)} onBlur={() => setTimeout(() => setFocused(false), 120)} />
      {open && <SuggestionList className="query-pop" rows={popupRows(needle, scopeField, suggestions)} active={active}
        onHover={(index) => index >= offset && setActive(index)}
        onPick={(index, event) => index < offset ? confirm() : pick(suggestions[index - offset], event.shiftKey)}
        empty={!suggestions.length && scopeField && <>No {FIELD_TITLES[scopeField].toLowerCase()} contain “{needle}”</>}
        footer={typing.exclude
          ? <span><kbd>Enter</kbd> leave out</span>
          : <><span><kbd>Enter</kbd> require</span><span><kbd>Shift+Enter</kbd> leave out</span><span><kbd>-tag:</kbd> leaves out when typed</span></>} />}
    </div>
    <button type="button" className={`filter-toggle${filtersOpen ? " is-open" : ""}`} aria-expanded={filtersOpen} onClick={onToggleFilters}>
      <SlidersHorizontal size={16} aria-hidden="true" />Filters{chips.length > 0 && <span className="filter-toggle__badge">{chips.length}</span>}
    </button>
  </div>;
}

/** The popup's rows: "search for the text" (absent when a field is typed), then the suggestions under their field's title. */
function popupRows(needle: string, scopeField: ExcludableField | undefined, suggestions: Suggestion[]): SuggestionRow[] {
  const search: SuggestionRow[] = scopeField ? [] : [{ key: "search", heading: <>Search for “{needle}”</>, icon: <Search size={14} />, label: <>“{needle}” in names and tags</>, hint: "Enter" }];
  return [...search, ...suggestions.map((row, index): SuggestionRow => ({
    key: `${row.field}:${row.value}`,
    heading: index === 0 || suggestions[index - 1].field !== row.field ? FIELD_TITLES[row.field] : undefined,
    icon: row.field === "tags" ? <TagIcon size={14} /> : <span className="query-pop__dot" />,
    label: row.field === "formats" ? formatLabel(row.value) : row.value,
    count: row.count,
  }))];
}
