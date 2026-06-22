/**
 * First-run / re-point gate: prompts the user to choose their asset library
 * folder. Shown whenever no library is configured (and reused by the toolbar's
 * "Change library…" action). On a successful pick the parent re-initializes the
 * catalog; a rejected folder (not a Toybox library) is surfaced inline.
 */

import { useState } from "react";

import { pickDirectory } from "../services/pickDirectory";
import { setLibraryRoot } from "../services/tauriApi";
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

  const choose = async () => {
    const dir = await pickDirectory();
    if (!dir) return;
    setBusy(true);
    setError(null);
    try {
      await setLibraryRoot(dir);
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
        <strong>Choose your asset library</strong>
        <span style={{ color: "var(--text-2)", textAlign: "center", maxWidth: 420 }}>
          Pick the folder that contains your Toybox library
          (it should have a <code>_library_config/catalog.json</code> inside).
        </span>
      </Stack>
      {error && <span style={{ color: "var(--danger)" }}>{error}</span>}
      <Stack dir="row" gap={8}>
        {onCancel && (
          <Button onClick={onCancel} variant="ghost">
            Cancel
          </Button>
        )}
        <Button onClick={() => void choose()}>Choose folder…</Button>
      </Stack>
    </Stack>
  );
}
