import { ExternalLink } from "lucide-react";
import { Field } from "@ldlework/toybox-sdk/ui";

import type { Asset } from "../domain/catalog";
import { assetFacets } from "../domain/assetCard";
import { openExternalUrl } from "../services/openExternal";
import { FacetLink, type OnFacet } from "./FacetLink";

/** The item's publisher, source, license and category, each labeled and each a filter; the source carries the
 *  link to its web page. */
export function AssetFacets({ asset, onFacet }: { asset: Asset; onFacet: OnFacet }) {
  const url = asset.user.provenance?.url;
  return <>{assetFacets(asset).map(({ field, label, value }) => <Field key={field} label={label}>
    <FacetLink field={field} value={value} onFacet={onFacet} />
    {field === "sources" && url && <button type="button" className="asset-card__url" title={url} aria-label="Open the source page" onClick={() => void openExternalUrl(url)}><ExternalLink size={12} aria-hidden="true" /></button>}
  </Field>)}</>;
}
