/**
 * The query bar's language: free text plus field terms, the same conditions the filter pane sets.
 *
 *   door tag:sci-fi -publisher:"Kitbash Works" licence:cc0 is:favorite
 *
 * `field:value` chooses a value, `-field:value` leaves it out, a value with spaces is quoted. Words that are not
 * terms (and terms of an unknown field) stay search text. Pure: no runtime imports beyond the filter helpers.
 */

import { EXCLUDABLE, withFacet, type ExcludableField, type Filter } from "./facets";

/** The words a field is typed as, first one is how it is written back. */
export const FIELD_WORDS: Record<ExcludableField, string[]> = {
  tags: ["tag", "tags"],
  publishers: ["publisher", "pub"],
  sources: ["source", "src"],
  licenses: ["licence", "license"],
  formats: ["format", "fmt"],
  categories: ["category", "cat"],
};

const FIELD_OF = new Map<string, ExcludableField>(EXCLUDABLE.flatMap((field) => FIELD_WORDS[field].map((word) => [word, field] as const)));

export interface QueryTerm {
  field: ExcludableField;
  value: string;
  exclude: boolean;
}

export interface ParsedQuery {
  terms: QueryTerm[];
  /** `is:favorite` was typed. */
  favorites: boolean;
  /** What is left: the search text, words separated by one space. */
  text: string;
}

/** Split into words, keeping quoted parts (`"a b"`, `tag:"a b"`) together and dropping the quotes. */
function words(input: string): string[] {
  const out: string[] = [];
  let word = "";
  let quoted = false;
  let started = false;
  for (const char of input) {
    if (char === '"') { quoted = !quoted; started = true; continue; }
    if (!quoted && /\s/.test(char)) {
      if (started) out.push(word);
      word = "";
      started = false;
      continue;
    }
    word += char;
    started = true;
  }
  if (started) out.push(word);
  return out;
}

export function parseQuery(input: string): ParsedQuery {
  const terms: QueryTerm[] = [];
  const text: string[] = [];
  let favorites = false;
  for (const word of words(input)) {
    const exclude = word.startsWith("-") && word.length > 1;
    const body = exclude ? word.slice(1) : word;
    const colon = body.indexOf(":");
    const name = colon > 0 ? body.slice(0, colon).toLowerCase() : "";
    const value = colon > 0 ? body.slice(colon + 1).trim() : "";
    const field = FIELD_OF.get(name);
    if (field && value) { terms.push({ field, value, exclude }); continue; }
    if (name === "is" && /^fav(ou?rites?)?$/i.test(value) && !exclude) { favorites = true; continue; }
    if (word.trim()) text.push(word);
  }
  return { terms, favorites, text: text.join(" ") };
}

/** The word being typed (the last one of `draft`): the field it is scoped to when it reads `field:part` with a known
 *  field, the text after the colon, and whether it began with `-` (a value picked for it is left out). */
export interface TypingTerm { field?: ExcludableField; part: string; exclude: boolean }

export function typingTerm(draft: string): TypingTerm {
  const last = draft.split(/\s+/).pop() ?? "";
  const exclude = last.startsWith("-");
  const scoped = /^-?([a-z]+):(.*)$/i.exec(last);
  const field = scoped ? FIELD_OF.get(scoped[1].toLowerCase()) : undefined;
  return field ? { field, part: scoped![2], exclude } : { part: last, exclude };
}

/** `filter` with the terms of `query` applied and its text as the search text. */
export function applyQuery(filter: Filter, query: ParsedQuery): Filter {
  let next: Filter = { ...filter, text: query.text, favoritesOnly: filter.favoritesOnly || query.favorites };
  for (const term of query.terms) next = withFacet(next, term.field, term.value, term.exclude ? "out" : "in");
  return next;
}

/** How a value is typed back (quoted when it has a space or a quote-breaking character). */
export const quoteValue = (value: string): string => (/[\s"]/.test(value) || value === "" ? `"${value.split('"').join("")}"` : value);

/** One condition of the filter as the query bar shows it. */
export interface FilterChip {
  /** `facet`: a value of a field; `range`: a property's bounds; `favorites`. */
  kind: "facet" | "range" | "favorites";
  field?: ExcludableField;
  value?: string;
  exclude?: boolean;
  /** Range chips: the property path. */
  path?: string;
  /** Short field name (`tag`, `publisher`). */
  label: string;
  /** What is shown after it. */
  text: string;
}

/** The chips of `filter`, in field order, chosen before left-out values. `properties` names range paths. */
export function filterChips(filter: Filter, properties: { path: string; label: string; unit?: string }[] = []): FilterChip[] {
  const chips: FilterChip[] = [];
  if (filter.favoritesOnly) chips.push({ kind: "favorites", label: "is", text: "favorite" });
  for (const field of EXCLUDABLE) {
    const label = FIELD_WORDS[field][0];
    for (const value of [...filter[field]].sort()) chips.push({ kind: "facet", field, value, exclude: false, label, text: value || "Unknown" });
    for (const value of [...filter.exclude[field]].sort()) chips.push({ kind: "facet", field, value, exclude: true, label, text: value || "Unknown" });
  }
  for (const [path, { min, max }] of Object.entries(filter.ranges).sort(([a], [b]) => a.localeCompare(b))) {
    const property = properties.find((known) => known.path === path);
    const unit = property?.unit ? ` ${property.unit}` : "";
    const bounds = min !== undefined && max !== undefined ? `${min} – ${max}` : min !== undefined ? `≥ ${min}` : max !== undefined ? `≤ ${max}` : "any";
    chips.push({ kind: "range", path, label: (property?.label ?? path).toLowerCase(), text: `${bounds}${unit}` });
  }
  return chips;
}
