import { variantInfo } from "@ldlework/toybox-sdk";
import type { ReviewItem } from "./edit.js";

export function materialGroups(items: ReviewItem[]): number[][] {
  const groups = new Map<string, number[]>();
  items.forEach((item, index) => {
    const info = variantInfo(item.draft.name, "material");
    const key = `${item.draft.dir.replace(/(^|\/)(1k|2k|4k|8k|512|1024|2048|4096|8192)(?=\/|$)/ig, "$1")}\0${info.base.toLowerCase()}`;
    const group = groups.get(key) ?? [];
    group.push(index); groups.set(key, group);
  });
  return [...groups.values()].map((group) => group.sort((a, b) => variantInfo(items[a].draft.name, "material").rank - variantInfo(items[b].draft.name, "material").rank));
}
