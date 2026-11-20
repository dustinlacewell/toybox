/** The saved views of this computer (see `domain/savedViews`), kept with the app's other preferences. */

import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { Filter } from "../domain/facets";
import { storeFilter, type SavedView } from "../domain/savedViews";

interface SavedViewsState {
  views: SavedView[];
  save: (name: string, filter: Filter, collection: string | null) => void;
  remove: (id: string) => void;
}

export const useSavedViews = create(persist<SavedViewsState>((set) => ({
  views: [],
  save: (name, filter, collection) => set((state) => {
    const existing = state.views.find((view) => view.name === name);
    const view: SavedView = { id: existing?.id ?? `view-${Date.now().toString(36)}`, name, filter: storeFilter(filter), collection };
    return { views: existing ? state.views.map((known) => (known.id === existing.id ? view : known)) : [...state.views, view] };
  }),
  remove: (id) => set((state) => ({ views: state.views.filter((view) => view.id !== id) })),
}), {
  name: "toybox-saved-views",
  merge: (saved, current) => {
    const views = (saved as { views?: unknown } | undefined)?.views;
    return { ...current, views: Array.isArray(views) ? views.filter((view): view is SavedView => !!view && typeof view.id === "string" && typeof view.name === "string" && typeof view.filter === "object") : [] };
  },
}));
