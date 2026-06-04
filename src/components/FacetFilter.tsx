/**
 * Left-rail facet filter: pack and category checkboxes with live counts, plus a
 * favorites-only toggle. Counts come from the pure selectors in domain/facets.
 */

import type { Asset, Category, Pack, PackMeta } from "../domain/catalog";
import { categoryCounts, packCounts, type Filter } from "../domain/facets";
import { Checkbox } from "../ds/Checkbox";
import { Button } from "../ds/Button";
import "./FacetFilter.css";

interface Props {
  assets: Asset[];
  filter: Filter;
  /** Pack identity metadata by slug (from each pack's pack.json). */
  packMeta: Map<string, PackMeta>;
  onTogglePack: (pack: Pack) => void;
  onToggleCategory: (category: Category) => void;
  onToggleFavoritesOnly: () => void;
  onToggleAnimatedOnly: () => void;
  onClear: () => void;
}

export function FacetFilter({
  assets,
  filter,
  packMeta,
  onTogglePack,
  onToggleCategory,
  onToggleFavoritesOnly,
  onToggleAnimatedOnly,
  onClear,
}: Props) {
  const packs = packCounts(assets);
  const categories = categoryCounts(assets);

  // Display name from pack.json; fall back to a title-cased slug so a pack
  // missing its metadata file never renders blank.
  const packLabel = (slug: string) => packMeta.get(slug)?.name ?? titleCaseSlug(slug);

  return (
    <aside className="facets">
      <div className="facets__section">
        <Checkbox
          checked={filter.favoritesOnly}
          onChange={onToggleFavoritesOnly}
          label={<strong>Favorites only</strong>}
        />
        <Checkbox
          checked={filter.animatedOnly}
          onChange={onToggleAnimatedOnly}
          label={<strong>Animated only</strong>}
        />
      </div>

      <FacetGroup title="Pack">
        {packs.map(({ value, count }) => (
          <FacetRow
            key={value}
            label={packLabel(value)}
            count={count}
            checked={filter.packs.has(value)}
            onToggle={() => onTogglePack(value)}
          />
        ))}
      </FacetGroup>

      <FacetGroup title="Category">
        {categories.map(({ value, count }) => (
          <FacetRow
            key={value}
            label={titleCase(value)}
            count={count}
            checked={filter.categories.has(value)}
            onToggle={() => onToggleCategory(value)}
          />
        ))}
      </FacetGroup>

      <div className="facets__section">
        <Button variant="ghost" onClick={onClear}>
          Clear filters
        </Button>
      </div>
    </aside>
  );
}

function FacetGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="facets__section">
      <div className="facets__title">{title}</div>
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
  onToggle: () => void;
}) {
  return (
    <div className="facets__row">
      <Checkbox checked={checked} onChange={onToggle} label={label} />
      <span className="facets__count">{count}</span>
    </div>
  );
}

const titleCase = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** Fallback pack label when no pack.json: `polygon_scifi_space` → `Polygon Scifi Space`. */
const titleCaseSlug = (slug: string) =>
  slug.split(/[_-]+/).map(titleCase).join(" ");
