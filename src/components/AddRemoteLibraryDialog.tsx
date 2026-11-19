/**
 * Add a library that lives on a server: its address and an access token. The
 * "Connect" step asks the server who it is and what the token allows, and says
 * so ("Connected to My Toybox as read-only") before anything is saved.
 */

import { useMemo, useState } from "react";
import { Button, Dialog, TextInput } from "@ldlework/toybox-sdk/ui";

import type { LibraryInfo } from "../domain/catalog";
import { connectedLine, roleSummary } from "../domain/remoteLibrary";
import { normalizeServerUrl } from "../domain/remoteUrl";
import { addRemoteLibrary, probeRemoteLibrary, type RemoteProbe } from "../services/tauriApi";
import "./RemoteLibrary.css";

const reason = (failure: unknown) => String(failure).replace(/^Error: /, "");

export function AddRemoteLibraryDialog({ onAdded, onClose }: { onAdded: (library: LibraryInfo) => void | Promise<void>; onClose: () => void }) {
  const [address, setAddress] = useState("");
  const [token, setToken] = useState("");
  const [probe, setProbe] = useState<RemoteProbe>();
  const [busy, setBusy] = useState<"connect" | "add">();
  const [error, setError] = useState<string>();
  const parsed = useMemo(() => normalizeServerUrl(address), [address]);
  const addressProblem = address.trim() && !parsed.ok ? parsed.error : undefined;
  const warning = probe?.warning ?? (parsed.ok ? parsed.warning : undefined);
  const canConnect = parsed.ok && token.trim().length > 0 && !busy;

  const change = (edit: () => void) => { edit(); setProbe(undefined); setError(undefined); };
  const connect = async () => {
    setBusy("connect"); setError(undefined);
    try { setProbe(await probeRemoteLibrary(address, token)); }
    catch (failure) { setError(reason(failure)); }
    finally { setBusy(undefined); }
  };
  const add = async () => {
    setBusy("add"); setError(undefined);
    try { const library = await addRemoteLibrary(address, token); await onAdded(library); onClose(); }
    catch (failure) { setError(reason(failure)); setBusy(undefined); }
  };

  const footer = <>
    <Button disabled={busy === "add"} onClick={onClose}>Cancel</Button>
    {probe
      ? <Button variant="primary" disabled={!!busy} onClick={() => void add()}>{busy === "add" ? "Adding…" : "Add library"}</Button>
      : <Button variant="primary" disabled={!canConnect} onClick={() => void connect()}>{busy === "connect" ? "Connecting…" : "Connect"}</Button>}
  </>;

  return <Dialog open size="compact" title="Add remote library" onClose={() => { if (!busy) onClose(); }} footer={footer}>
    <div className="remote-dialog" aria-busy={!!busy}>
      <label className="remote-dialog__field"><span>Server address</span>
        <TextInput autoFocus value={address} placeholder="https://assets.example.com" disabled={!!busy} spellCheck={false} autoComplete="off"
          onChange={(event) => { const value = event.currentTarget.value; change(() => setAddress(value)); }} />
      </label>
      {addressProblem && <p className="remote-dialog__error" role="alert">{addressProblem}</p>}
      {!addressProblem && warning && <p className="remote-dialog__warning" role="note">{warning}</p>}
      <label className="remote-dialog__field"><span>Access token</span>
        <TextInput type="password" value={token} placeholder="tbx_…" disabled={!!busy} spellCheck={false} autoComplete="off"
          onChange={(event) => { const value = event.currentTarget.value; change(() => setToken(value)); }}
          onKeyDown={(event) => { if (event.key === "Enter" && canConnect && !probe) void connect(); }} />
      </label>
      <p className="remote-dialog__note">The token is kept in your system's credential store, not in Toybox's files.</p>
      {probe && <>
        <p className="remote-dialog__ok" role="status">{connectedLine(probe.serverName, probe.role)}{probe.notice ? ` ${probe.notice}` : ""}</p>
        <p className="remote-dialog__note">{roleSummary(probe.role)}</p>
      </>}
      {error && <p className="remote-dialog__error" role="alert">{error}</p>}
    </div>
  </Dialog>;
}
