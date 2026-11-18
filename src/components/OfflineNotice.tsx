import { useState } from "react";
import { Button } from "@ldlework/toybox-sdk/ui";

import type { LibraryInfo } from "../domain/catalog";
import { syncLibrary } from "../services/libraries";
import { reconnectRemoteLibrary } from "../services/tauriApi";
import "./RemoteLibrary.css";

/** Under the grid: remote libraries whose server cannot be reached are shown
 *  from their saved catalog, read-only. "Try again" asks the server once more. */
export function OfflineNotice({ libraries }: { libraries: LibraryInfo[] }) {
  const [trying, setTrying] = useState<string>();
  const offline = libraries.filter((library) => library.remote && (library.offline || library.remote.offline));
  if (!offline.length) return null;
  const retry = async (id: string) => {
    setTrying(id);
    try { await reconnectRemoteLibrary(id); await syncLibrary(id, false); }
    catch { /* the notice stays and says why */ }
    finally { setTrying(undefined); }
  };
  return <div className="library-notice library-notice--offline" role="status">
    {offline.map((library) => <span key={library.id} className="library-notice__offline">
      <span>{library.name} is offline, so its saved catalog is shown and it cannot be changed.{library.offlineReason ? ` ${library.offlineReason}` : ""}</span>
      <Button variant="ghost" disabled={!!trying} onClick={() => void retry(library.id)}>{trying === library.id ? "Trying…" : "Try again"}</Button>
    </span>)}
  </div>;
}
