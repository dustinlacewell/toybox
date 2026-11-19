/**
 * What a remote library's expanded settings panel shows beyond a local one's,
 * as rows of the panel's definition list: the access level in words, the offline
 * reason (with "Try again"), the access actions, and the size of the downloaded
 * files (with "Clear downloaded files").
 */

import { useCallback, useEffect, useState } from "react";
import { Button } from "@ldlework/toybox-sdk/ui";

import type { LibraryInfo } from "../domain/catalog";
import { canManageAccess, formatBytes, versionNotice } from "../domain/remoteLibrary";
import { accessSentence } from "../domain/librarySettings";
import { clearRemoteCache, reconnectRemoteLibrary, remoteCacheSize } from "../services/tauriApi";
import { ChangeTokenDialog, ManageAccessDialog } from "./RemoteAccessDialogs";
import "./RemoteLibrary.css";

export function RemoteLibraryDetails({ library, busy, run, onChanged }: {
  library: LibraryInfo;
  busy: string | undefined;
  /** Runs `work` as the settings page's busy action (shows failures there). */
  run: (key: string, work: () => Promise<void>) => Promise<void>;
  /** Re-read the library after its connection changed. */
  onChanged: () => Promise<void>;
}) {
  const [size, setSize] = useState<number>();
  const [changing, setChanging] = useState(false);
  const [managing, setManaging] = useState(false);
  const refreshSize = useCallback(() => { remoteCacheSize(library.id).then(setSize, () => setSize(undefined)); }, [library.id]);
  useEffect(refreshSize, [refreshSize]);
  if (!library.remote) return null;
  const offline = !!(library.offline || library.remote.offline);

  return <>
    <dt>Access</dt>
    <dd>{accessSentence(library.remote.role)}
      <div className="libraries__inline">
        <Button variant="ghost" disabled={!!busy} onClick={() => setChanging(true)}>Change token…</Button>
        {canManageAccess(library) && <Button variant="ghost" disabled={!!busy} onClick={() => setManaging(true)}>Manage access…</Button>}
      </div>
    </dd>
    {versionNotice(library) && <><dt>Versions</dt>
      <dd><span className={library.remote.compat === "notice" ? "libraries__why" : "libraries__problem"} role={library.remote.compat === "notice" ? "status" : "alert"}>{versionNotice(library)}</span></dd></>}
    {offline && <><dt>Connection</dt>
      <dd><span className="libraries__problem" role="alert">{library.offlineReason ?? "The server cannot be reached right now."}</span> Your cached copy of the catalog is shown and nothing can be changed until it is back.
        <div className="libraries__inline">
          <Button disabled={!!busy} onClick={() => void run(`retry:${library.id}`, async () => { await reconnectRemoteLibrary(library.id); await onChanged(); })}>{busy === `retry:${library.id}` ? "Trying…" : "Try again"}</Button>
        </div>
      </dd></>}
    <dt>Downloaded files</dt>
    <dd>{size === undefined ? "…" : formatBytes(size)}. Files are fetched when you open an item and kept here; they never go stale.
      <div className="libraries__inline">
        <Button variant="ghost" disabled={!!busy || !size} onClick={() => void run(`clear:${library.id}`, async () => { await clearRemoteCache(library.id); refreshSize(); })}>{busy === `clear:${library.id}` ? "Clearing…" : "Clear downloaded files"}</Button>
      </div>
    </dd>
    {changing && <ChangeTokenDialog library={library} onChanged={onChanged} onClose={() => setChanging(false)} />}
    {managing && <ManageAccessDialog library={library} onClose={() => setManaging(false)} />}
  </>;
}
