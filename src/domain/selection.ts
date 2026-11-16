/** Range selection follows the filtered display order, including virtual rows. */
export function selectItems(order: string[], selected: ReadonlySet<string>, anchor: string | null, target: string, mode: "replace" | "toggle" | "range") {
  if (mode === "replace") return { ids: [target], anchor: target };
  const next = new Set(selected);
  if (mode === "toggle") {
    next.has(target) ? next.delete(target) : next.add(target);
    return { ids: [...next], anchor: target };
  }
  const start = anchor === null ? -1 : order.indexOf(anchor), end = order.indexOf(target);
  if (start < 0 || end < 0) return { ids: [...next.add(target)], anchor: target };
  for (const id of order.slice(Math.min(start, end), Math.max(start, end) + 1)) next.add(id);
  return { ids: [...next], anchor };
}
