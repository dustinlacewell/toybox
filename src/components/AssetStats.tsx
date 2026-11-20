import { Field, formatTime } from "@ldlework/toybox-sdk/ui";

import type { Asset } from "../domain/catalog";
import { assetStats, rigNodeCount, type StatAction } from "../domain/assetCard";
import { pluginProperties } from "../state/assetTypes";
import { useStore } from "../state/store";

/** The item's facts, each labeled; one that stands for more opens the full view. */
export function AssetStats({ asset, onOpen }: { asset: Asset; onOpen: () => void }) {
  const rigs = useStore((state) => state.rigs);
  const stats = assetStats({
    asset,
    variants: [asset, ...(asset.variants ?? [])],
    rigNodes: rigNodeCount(asset.rigs, rigs),
    properties: asset.kind === "plugin" ? pluginProperties(asset) : undefined,
  }, { time: formatTime });
  const actions: Record<StatAction, () => void> = { open: onOpen };
  return <>{stats.map((stat) => <Field key={stat.id} label={stat.caption}>
    {stat.press ? <button type="button" className="facet-link" title="Open the full view" onClick={actions[stat.press]}>{stat.value}</button> : stat.value}
  </Field>)}</>;
}
