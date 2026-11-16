import { useEffect } from "react";
import { variantInfo } from "@ldlework/toybox-sdk";
import type { Asset } from "../domain/catalog";

export function VariantSwitch({ variants, selected, onSelect }: { variants: Asset[]; selected: number; onSelect: (index: number) => void }) {
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if ((event.target as HTMLElement).closest("input, textarea, select, [contenteditable=true]")) return;
      const index = Number(event.key) - 1;
      if (index >= 0 && index < variants.length && !event.ctrlKey && !event.metaKey && !event.altKey) { event.preventDefault(); onSelect(index); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [variants.length, onSelect]);
  return <div className="variant-switch" role="group" aria-label={variants[0]?.kind === "model" ? "Detail levels" : "Sizes"}>
    {variants.map((v, i) => <button key={v.id} aria-pressed={selected === i} title={`Shortcut ${i + 1}`} onClick={() => onSelect(i)}>{variantInfo(v.fileName, v.kind).label}</button>)}
  </div>;
}
