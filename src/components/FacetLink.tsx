import type { MouseEvent } from "react";
import type { ExcludableField } from "../domain/facets";

/** Require (or, with Shift, leave out) a value of a filter field. */
export type OnFacet = (field: ExcludableField, value: string, exclude: boolean) => void;

/** The title of a value that filters when pressed. */
export const facetTitle = (value: string): string => `Show only ${value} · Shift-click to leave out`;

/** A shown value that filters on click; Shift-click leaves it out. */
export function FacetLink({ field, value, onFacet }: { field: ExcludableField; value: string; onFacet: OnFacet }) {
  const press = (event: MouseEvent) => onFacet(field, value, event.shiftKey);
  return <button type="button" className="facet-link" title={facetTitle(value)} onClick={press}>{value}</button>;
}
