/**
 * Metadata filters with live counts, supplied to controlled design-system inputs.
 */

import type { Asset, Category } from "../domain/catalog";
import { categoryCounts, ofKind, provenanceCounts, isProvenanceTag, tagCounts, type Filter } from "../domain/facets";
import { Button, IconButton } from "@ldlework/toybox-sdk/ui";
import { Star, FilterX } from "lucide-react";
import "./FacetFilter.css";

interface Props {
  assets: Asset[];
  filter: Filter;
  onToggleSource: (source: string, additive: boolean) => void;
  onTogglePublisher: (publisher: string, additive: boolean) => void;
  onToggleLicense: (license: string, additive: boolean) => void;
  onToggleCategory: (category: Category, additive: boolean) => void;
  onToggleTag?: (tag:string, additive: boolean) => void;
  onToggleFavoritesOnly: () => void;
  onClear: () => void;
}

export function FacetFilter({
  assets,
  filter,
  onToggleSource,
  onTogglePublisher,
  onToggleLicense,
  onToggleCategory,
  onToggleTag,
  onToggleFavoritesOnly,
  onClear,
}: Props) {
  // Counts cover only the current type.
  const ofCurrentKind = ofKind(assets, filter.kind);
  const categories = categoryCounts(ofCurrentKind);
  const tags=tagCounts(ofCurrentKind).filter(({value})=>!isProvenanceTag(value));

  return (
    <aside className="facets">
      <div className="facets__toolbar">
        <Button variant="ghost" className="facets__row facets__favorites" aria-pressed={filter.favoritesOnly} onClick={onToggleFavoritesOnly}><Star size={16} aria-hidden="true"/>Favorites</Button>
        <IconButton icon={FilterX} label="Clear filters" onClick={onClear}/>
      </div>

      {([{field:"source",title:"Sources",selected:filter.sources,onToggle:onToggleSource},
        {field:"publisher",title:"Publishers",selected:filter.publishers,onToggle:onTogglePublisher},
        {field:"license",title:"Licenses",selected:filter.licenses,onToggle:onToggleLicense}] as const).map(group=><FacetGroup key={group.field} title={group.title}>
          {provenanceCounts(ofCurrentKind,group.field).map(({value,count})=><FacetRow key={value} label={value||"Unknown"} count={count} checked={group.selected.has(value)} onToggle={additive=>group.onToggle(value,additive)}/>)}
      </FacetGroup>)}
      {onToggleTag&&!!tags.length&&<FacetGroup title="Tags">{tags.map(({value,count})=><FacetRow key={value} label={value} count={count} checked={filter.tags.has(value)} onToggle={additive=>onToggleTag(value,additive)}/>)}</FacetGroup>}

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
}

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
  count,
  checked,
  onToggle,
}: {
  label: string;
  count: number;
  checked: boolean;
  onToggle: (additive: boolean) => void;
}) {
  return (
    <Button variant="ghost" className="facets__row" aria-pressed={checked} onClick={event=>onToggle(event.shiftKey)} onMouseDown={event=>{if(event.shiftKey)event.preventDefault();}}>
      <span className="facets__label" title={label}>{label}</span>
      <span className="facets__count">{count}</span>
    </Button>
  );
}

const titleCase = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
