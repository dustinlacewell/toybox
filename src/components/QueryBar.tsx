/**
 * The query bar: every condition of the filter as a chip, and the search text. Typing searches as it goes; a
 * typed field term (`tag:sci-fi`, `-publisher:"Kitbash Works"`) becomes a chip on Enter. While typing, the values
 * of the fields that contain the text are suggested: Enter or a click chooses one, Shift leaves it out.
 */

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Search, SlidersHorizontal, Tag as TagIcon, X } from "lucide-react";

import { EXCLUDABLE, formatLabel, type ExcludableField, type Filter } from "../domain/facets";
import { FIELD_WORDS, filterChips, parseQuery, type FilterChip } from "../domain/query";
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
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  // The filter's text changes elsewhere too (a cleared filter, a saved view).
  useEffect(() => { if (parseQuery(draft).text !== filter.text) setDraft(filter.text); }, [filter.text]); // eslint-disable-line react-hooks/exhaustive-deps

  // The word being typed: `field:part` narrows the suggestions to that field.
  const last = draft.split(/\s+/).pop() ?? "";
  const scoped = /^-?([a-z]+):(.*)$/i.exec(last);
  const scopeField = scoped ? EXCLUDABLE.find((field) => FIELD_WORDS[field].includes(scoped[1].toLowerCase())) : undefined;
  const needle = (scopeField ? scoped![2] : parseQuery(draft).text).replace(/"/g, "").trim().toLowerCase();
  const foundTags = useTagSearch(filter.kind, rules, needle.length >= 1 && (!scopeField || scopeField === "tags") ? needle : "");

  const suggestions = useMemo((): Suggestion[] => {
    if (!needle) return [];
    const from = (field: ExcludableField, rows: Count[] | undefined): Suggestion[] =>
      (rows ?? []).filter((row) => row.value && row.value.toLowerCase().includes(needle) && !filter[field].has(row.value) && !filter.exclude[field].has(row.value))
        .sort((a, b) => b.count - a.count).slice(0, PER_FIELD).map((row) => ({ field, value: row.value, count: row.count }));
    const fields: [ExcludableField, Count[] | undefined][] = [
      ["tags", foundTags], ["publishers", counts?.publishers], ["sources", counts?.sources],
      ["licenses", counts?.licenses], ["formats", counts?.formats], ["categories", counts?.categories],
    ];
    return fields.filter(([field]) => !scopeField || field === scopeField).flatMap(([field, rows]) => from(field, rows));
  }, [needle, scopeField, foundTags, counts, filter]);
  const open = focused && !!needle;
  // Row 0 is "search for the text" (absent when the word is a field term).
  const offset = scopeField ? 0 : 1;
  useEffect(() => setActive(0), [needle]);

  const chips = filterChips(filter, properties);
  const typed = (text: string) => { setDraft(text); onText(parseQuery(text).text); };
  /** Take the word being typed out of the draft and choose (or leave out) a suggestion instead. */
  const pick = (suggestion: Suggestion, exclude: boolean) => {
    const rest = draft.split(/\s+/).slice(0, -1).join(" ");
    onFacet(suggestion.field, suggestion.value, exclude ? "out" : "in");
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
    const rows = suggestions.length + offset;
    if (event.key === "ArrowDown" && open) { event.preventDefault(); setActive((at) => (at + 1) % Math.max(rows, 1)); }
    else if (event.key === "ArrowUp" && open) { event.preventDefault(); setActive((at) => (at - 1 + Math.max(rows, 1)) % Math.max(rows, 1)); }
    else if (event.key === "Enter") {
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
      {open && <div className="query-pop" role="listbox" onMouseDown={(event) => event.preventDefault()}>
        {!scopeField && <>
          <div className="query-pop__label">Search for “{needle}”</div>
          <button type="button" className={`query-pop__item${active === 0 ? " is-active" : ""}`} onClick={confirm}><Search size={14} /><span>“{needle}” in names and tags</span><kbd>Enter</kbd></button>
        </>}
        {EXCLUDABLE.map((field) => {
          const rows = suggestions.filter((suggestion) => suggestion.field === field);
          if (!rows.length) return null;
          return <div key={field}>
            <div className="query-pop__label">{FIELD_TITLES[field]}</div>
            {rows.map((row) => {
              const index = suggestions.indexOf(row) + offset;
              return <button type="button" key={row.value} className={`query-pop__item${active === index ? " is-active" : ""}`} onMouseEnter={() => setActive(index)}
                onClick={(event) => pick(row, event.shiftKey)}>
                {field === "tags" ? <TagIcon size={14} /> : <span className="query-pop__dot" />}
                <span className="query-pop__value">{field === "formats" ? formatLabel(row.value) : row.value}</span>
                <span className="query-pop__count">{row.count.toLocaleString()}</span>
              </button>;
            })}
          </div>;
        })}
        {!suggestions.length && scopeField && <div className="query-pop__empty">No {FIELD_TITLES[scopeField].toLowerCase()} contain “{needle}”</div>}
        <div className="query-pop__footer"><span><kbd>Enter</kbd> require</span><span><kbd>Shift+Enter</kbd> leave out</span><span><kbd>-tag:</kbd> leaves out when typed</span></div>
      </div>}
    </div>
    <button type="button" className={`filter-toggle${filtersOpen ? " is-open" : ""}`} aria-expanded={filtersOpen} onClick={onToggleFilters}>
      <SlidersHorizontal size={16} aria-hidden="true" />Filters{chips.length > 0 && <span className="filter-toggle__badge">{chips.length}</span>}
    </button>
  </div>;
}
