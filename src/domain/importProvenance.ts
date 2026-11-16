import type { ImportProvenance, SeedEntryInput } from "@ldlework/toybox-sdk";

export function normalizeProvenance(value:ImportProvenance):ImportProvenance|undefined {
  const entries=Object.entries(value).flatMap(([key,value])=>value?.trim()?[[key,value.trim()]]:[]);
  return entries.length?Object.fromEntries(entries):undefined;
}

/** Preserve plugin-specific source information and apply shared batch fields. */
export function withImportProvenance(entry:SeedEntryInput,batch:ImportProvenance):SeedEntryInput {
  const provenance=normalizeProvenance({...entry.provenance,...normalizeProvenance(batch)});
  const result={...entry,...(provenance?{provenance}:{})};
  if("variants" in entry&&entry.variants) {
    return {...result,variants:entry.variants.map(variant=>withImportProvenance(variant,batch))} as SeedEntryInput;
  }
  return result;
}
