/**
 * First-run / re-point gate: lets the user either open an existing asset library
 * or create a new, empty one. Shown whenever no library is configured (and
 * reused by the toolbar's "Change library…" action). On success the parent
 * re-initializes the catalog; a rejected folder is surfaced inline.
 *
 * Both verbs end at the same place — a validated, scoped, persisted root — so
 * they differ only in the command they commit through: open adopts an existing
 * Toybox library, create scaffolds an empty one first.
 */

import { useState } from "react";

import { pickDirectory } from "../services/pickDirectory";
import { createLibrary, setLibraryRoot } from "../services/tauriApi";
import { Button, Spinner, Stack } from "@ldlework/toybox-sdk/ui";

interface Props {
  /** Called after the library root is accepted, to (re)load the catalog. */
  onPicked: () => void;
  /** Optional cancel affordance (re-point flow); omitted on first run. */
  onCancel?: () => void;
}

export function LibraryPicker({ onPicked, onCancel }: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Pick a folder, commit it through the given command, and hand off on success.
  const run = (commit: (path: string) => Promise<void>) => async () => {
    const dir = await pickDirectory();
    if (!dir) return;
    setBusy(true);
    setError(null);
    try {
      await commit(dir);
      onPicked();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (busy) {
    return (
      <Stack grow align="center" justify="center" gap={12}>
        <Spinner />
        <span>Opening library…</span>
      </Stack>
    );
  }

  return (
    <Stack grow align="center" justify="center" gap={16}>
      <Stack align="center" gap={4}>
        <strong>Set up your asset library</strong>
        <span style={{ color: "var(--text-2)", textAlign: "center", maxWidth: 440 }}>
          Open an existing Toybox library, or create a new, empty one in a folder
          and add assets to it later via Import.
        </span>
      </Stack>
      {error && <span style={{ color: "var(--danger)" }}>{error}</span>}
      <Stack dir="row" gap={8}>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        )}
        <Button variant="ghost" onClick={() => void run(setLibraryRoot)()}>
          Open existing…
        </Button>
        <Button variant="primary" onClick={() => void run(createLibrary)()}>
          Create new…
        </Button>
      </Stack>
    </Stack>
  );
}
