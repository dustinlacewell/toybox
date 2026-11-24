/**
 * Items made from other items (`origin.derivedFrom`, see the SDK's `AssetOrigin`).
 * Pure: no runtime imports (loaded on their own by tests).
 */

/** Whether `asset` (or the variant) was made from the item `parentId`. Both ids
 *  in the same form: qualified, as the window holds them. */
export function isDerivedFrom(asset: { origin?: { derivedFrom?: string } }, parentId: string): boolean {
  return !!parentId && asset.origin?.derivedFrom === parentId;
}

/** The items of `assets` made from `parentId`: a cheap check of what a
 *  `derivedFrom` query answered, kept as a safety net. */
export const derivedOnly = <A extends { origin?: { derivedFrom?: string } }>(assets: readonly A[], parentId: string): A[] =>
  assets.filter((asset) => isDerivedFrom(asset, parentId));

/** Every item made from `parentId`, asking `page(offset, limit)` (a query
 *  filtered by `derivedFrom`) page after page until one comes back short, so the
 *  answer is never cut off part-way. Each page is checked with `derivedOnly`. */
export async function collectDerived<A extends { origin?: { derivedFrom?: string } }>(
  page: (offset: number, limit: number) => Promise<readonly A[]>,
  parentId: string,
  pageSize: number,
): Promise<A[]> {
  const found: A[] = [];
  for (let offset = 0; ; offset += pageSize) {
    const assets = await page(offset, pageSize);
    found.push(...derivedOnly(assets, parentId));
    if (assets.length < pageSize) return found;
  }
}
