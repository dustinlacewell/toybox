/**
 * Import panel. Lists discovered importer plugins; selecting one mounts its
 * `importPanel` slot component, which picks a source, builds seed entries, shows
 * its own preview, and commits. The commit is host-owned: `ctx.commit(entries)`
 * runs `merge_seed_entries` (opening the previously-closed inbound format) and
 * triggers a catalog refresh so the imported assets appear in the grid.
 *
 * Importers are wizard-shaped, so there is no declarative fallback — an importer
 * that ships no `importPanel` shows a note rather than an empty drawer.
 */

import { useState } from "react";

import { Drawer, Select } from "@ldlework/toybox-sdk/ui";
import type { ImportPanelCtx, SeedEntryInput } from "@ldlework/toybox-sdk";
import {
  type LoadedImporter,
  type PluginLoadError,
} from "../services/pluginRegistry";
import { buildSlotHost } from "../services/slotHost";
import { mergeSeedEntries } from "../services/tauriApi";
import { PluginSlot } from "./plugin-ui/PluginSlot";
import "./ExportDrawer.css";

interface Props {
  open: boolean;
  importers: LoadedImporter[];
  pluginErrors: PluginLoadError[];
  onClose: () => void;
  /** Called after a successful commit so the view can refresh the catalog. */
  onCommitted: () => void;
}

export function ImportDrawer({ open, importers, pluginErrors, onClose, onCommitted }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const active = importers.find((i) => i.manifest.id === selectedId);
  const panel = active?.manifest.ui?.importPanel;

  const ctx: ImportPanelCtx = {
    host: buildSlotHost(),
    commit: async (entries: SeedEntryInput[]) => {
      setError(null);
      try {
        await mergeSeedEntries(entries);
        onCommitted();
      } catch (e) {
        setError(String(e));
        throw e;
      }
    },
    close: onClose,
  };

  return (
    <Drawer open={open} onClose={onClose}>
      <div className="export">
        <header className="export__header">
          <h2>Import assets</h2>
          <button className="export__close" onClick={onClose}>✕</button>
        </header>

        <section className="export__section">
          <div className="export__label">Source</div>
          {importers.length === 0 ? (
            <div className="export__hint">
              No importer plugins installed. Drop one into the plugins folder to add a source.
            </div>
          ) : (
            <>
              <Select
                value={selectedId ?? ""}
                options={[
                  { value: "", label: "Choose a source…" },
                  ...importers.map((imp) => ({ value: imp.manifest.id, label: imp.manifest.name })),
                ]}
                onChange={(v) => setSelectedId(v || null)}
              />
              {active?.manifest.description && (
                <div className="export__hint">{active.manifest.description}</div>
              )}
            </>
          )}
          {pluginErrors.map((err) => (
            <div className="export__error" key={err.manifest.id}>
              Plugin “{err.manifest.name || err.manifest.id}” failed to load: {err.error}
            </div>
          ))}
        </section>

        {active && !panel && (
          <section className="export__section">
            <div className="export__hint">This importer ships no UI.</div>
          </section>
        )}

        {active && panel && (
          <PluginSlot
            key={active.manifest.id}
            pluginId={active.manifest.id}
            moduleRel={panel}
            ctx={ctx}
            fallback={null}
          />
        )}

        {error && <div className="export__error">{error}</div>}
      </div>
    </Drawer>
  );
}
