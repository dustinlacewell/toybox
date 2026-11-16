import { create } from "zustand";
import type { SavedExport } from "../domain/savedExport";
import { getSavedExports, setSavedExports } from "../services/tauriApi";

export const useSavedExports = create<{
  exports: SavedExport[];
  load: () => Promise<void>;
  save: (setup: SavedExport) => Promise<void>;
  remove: (id: string) => Promise<void>;
}>((set, get) => ({
  exports: [],
  load: async () => set({ exports: await getSavedExports() }),
  save: async (setup) => {
    const exports = [...get().exports.filter((e) => e.id !== setup.id), setup];
    await setSavedExports(exports);
    set({ exports });
  },
  remove: async (id) => {
    const exports = get().exports.filter((e) => e.id !== id);
    await setSavedExports(exports);
    set({ exports });
  },
}));
