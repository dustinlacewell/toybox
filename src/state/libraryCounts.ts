/**
 * The numbers beside the list: the sidebar's counts per type, the rail's counts for the type being shown, the
 * collections. Each is a query, asked again when the libraries change or the question does; the numbers
 * already on screen stay until the new ones arrive.
 */

import { useEffect, useMemo, useState } from "react";

import { rulesSignature, type VisibilityRules } from "../domain/browseQuery";
import { facetCounts, listCollections, searchTags, typeCounts, type Count, type FacetCounts } from "../services/browse";
import { useStore } from "./store";

/** Tags the rail shows before "Show more". */
export const TAG_ROWS = 60;

/** Run `ask` when `deps` change and keep the latest answer; an answer to an earlier question is dropped. */
function useQuery<T>(ask: () => Promise<T>, deps: unknown[]): T | undefined {
  const [value, setValue] = useState<T>();
  useEffect(() => {
    let alive = true;
    ask().then((answer) => { if (alive) setValue(answer); }, (failure) => console.warn("Could not read the library counts", failure));
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return value;
}

/** Items per type, each once, by type id. */
export function useTypeCounts(rules: VisibilityRules): Map<string, number> | undefined {
  const dataVersion = useStore((state) => state.dataVersion);
  const signature = rulesSignature(rules);
  const counts = useQuery(() => typeCounts(rules), [dataVersion, signature]);
  return useMemo(() => counts && new Map(counts.map(({ value, count }) => [value, count])), [counts]);
}

/** The rail's counts for one type. */
export function useFacetCounts(kind: string, rules: VisibilityRules, selectedTags: ReadonlySet<string>): FacetCounts | undefined {
  const dataVersion = useStore((state) => state.dataVersion);
  const signature = rulesSignature(rules);
  const tags = [...selectedTags].sort().join("\n");
  return useQuery(() => facetCounts(kind, rules, TAG_ROWS, [...selectedTags]), [dataVersion, signature, kind, tags]);
}

/** The collections and how many items of the type each holds. */
export function useCollections(kind: string, rules: VisibilityRules): { name: string; count: number }[] | undefined {
  const dataVersion = useStore((state) => state.dataVersion);
  const signature = rulesSignature(rules);
  return useQuery(() => listCollections(kind, rules), [dataVersion, signature, kind]);
}

/** The tags of a type that contain `text` (empty text: none), most used first. */
export function useTagSearch(kind: string, rules: VisibilityRules, text: string): Count[] | undefined {
  const dataVersion = useStore((state) => state.dataVersion);
  const signature = rulesSignature(rules);
  const needle = text.trim();
  const [found, setFound] = useState<Count[]>();
  useEffect(() => {
    if (!needle) { setFound(undefined); return; }
    let alive = true;
    // The box is typed in; the question follows a moment behind.
    const timer = setTimeout(() => { searchTags(kind, rules, needle, 200).then((tags) => { if (alive) setFound(tags); }, () => undefined); }, 150);
    return () => { alive = false; clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dataVersion, signature, kind, needle]);
  return needle ? found : undefined;
}
