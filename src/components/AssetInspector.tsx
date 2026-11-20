/**
 * The item in the open preview tray: its name over one row of labeled fields (type, the values that filter, its
 * facts, its library) and a row of tags and collections.
 */

import { Chip, Field } from "@ldlework/toybox-sdk/ui";
import { variantInfo } from "@ldlework/toybox-sdk";

import { assetType, type Asset } from "../domain/catalog";
import { assetTypeEntry } from "../state/assetTypes";
import { useStore } from "../state/store";
import { FavoriteStar } from "./FavoriteStar";
import { AssetFacets } from "./AssetFacets";
import { AssetLibraryNote } from "./AssetLibraryNote";
import { AssetStats } from "./AssetStats";
import { AssetTags } from "./AssetTags";
import type { OnFacet } from "./FacetLink";
import "./AssetInspector.css";

interface Props {
  asset: Asset;
  onOpen: () => void;
  onFavorite: () => void;
  onCollect: () => void;
  onFacet: OnFacet;
}

export function AssetInspector({ asset, onOpen, onFavorite, onCollect, onFacet }: Props) {
  const library = useStore((state) => state.libraries.find((entry) => entry.id === asset.library));
  const several = useStore((state) => state.libraries.length > 1);
  return <div className="asset-card">
    <div className="asset-card__title">
      <h2 className="asset-card__name">{variantInfo(asset.name, asset.kind).base}</h2>
      <FavoriteStar favorited={asset.user.favorite} onToggle={onFavorite} />
    </div>
    <div className="asset-card__fields">
      <Field label="Type">{assetTypeEntry(assetType(asset)).singular}</Field>
      <AssetFacets asset={asset} onFacet={onFacet} />
      <AssetStats asset={asset} onOpen={onOpen} />
      {several && <Field label="Library"><AssetLibraryNote asset={asset} /></Field>}
    </div>
    <div className="asset-card__fields">
      <Field label="Tags" className="asset-card__tags"><AssetTags asset={asset} library={library} onFacet={onFacet} suggestAbove /></Field>
      <Collections names={asset.user.collections ?? []} onCollect={onCollect} />
    </div>
  </div>;
}

/** The collections holding the item; pressing one opens the collection dialog. */
function Collections({ names, onCollect }: { names: readonly string[]; onCollect: () => void }) {
  if (!names.length) return null;
  return <Field label="Collections"><div className="asset-card__collections" role="list">
    {names.map((name) => <Chip key={name} role="listitem" tone="accent" label={name} title="Change collections" onPress={onCollect} />)}
  </div></Field>;
}
