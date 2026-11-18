import type { AssetKind } from "./catalog";

export interface SavedExport {
  id: string;
  name: string;
  kind: AssetKind;
  /** The library the setup belongs to. `lastAssets` is keyed by that library's raw asset ids. */
  libraryId: string;
  exporterId: string;
  targetDir: string;
  preserveStructure: boolean;
  config: Record<string, unknown>;
  variantPolicy: "all" | "best";
  lastSent?: string;
  lastAssets?: Record<string, string>;
}
