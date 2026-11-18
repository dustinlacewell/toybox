/**
 * Access to a remote library: "Change token…" (the token this computer uses)
 * and "Manage access…" (an admin lists, makes and revokes the server's tokens).
 * A new token is shown once, with a copy button and a plain warning.
 */

import { useCallback, useEffect, useState } from "react";
import { Button, ConfirmationDialog, Dialog, Select, TextInput } from "@ldlework/toybox-sdk/ui";

import type { LibraryInfo } from "../domain/catalog";
import { NEW_TOKEN_WARNING, TOKEN_ROLES, formatEpoch, roleLabel, roleSummary, type RemoteRole } from "../domain/remoteLibrary";
import { changeRemoteToken, createRemoteToken, listRemoteTokens, revokeRemoteToken, type RemoteToken } from "../services/tauriApi";
import "./RemoteLibrary.css";

const reason = (failure: unknown) => String(failure).replace(/^Error: /, "");

/** Enter another access token for this library; it is checked with the server before it is kept. */
export function ChangeTokenDialog({ library, onChanged, onClose }: { library: LibraryInfo; onChanged: () => void | Promise<void>; onClose: () => void }) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const save = async () => {
    setBusy(true); setError(undefined);
    try { await changeRemoteToken(library.id, token); await onChanged(); onClose(); }
    catch (failure) { setError(reason(failure)); setBusy(false); }
  };
  return <Dialog open size="compact" title={`Change token for "${library.name}"`} onClose={() => { if (!busy) onClose(); }}
    footer={<><Button disabled={busy} onClick={onClose}>Cancel</Button><Button variant="primary" disabled={busy || !token.trim()} onClick={() => void save()}>{busy ? "Checking…" : "Use this token"}</Button></>}>
    <div className="remote-dialog" aria-busy={busy}>
      <label className="remote-dialog__field"><span>New access token</span>
        <TextInput autoFocus type="password" value={token} placeholder="tbx_…" disabled={busy} spellCheck={false} autoComplete="off"
          onChange={(event) => { setToken(event.currentTarget.value); setError(undefined); }}
          onKeyDown={(event) => { if (event.key === "Enter" && token.trim() && !busy) void save(); }} />
      </label>
      <p className="remote-dialog__note">The server is asked whether it accepts the token before it replaces the old one. It is kept in your system's credential store.</p>
      {error && <p className="remote-dialog__error" role="alert">{error}</p>}
    </div>
  </Dialog>;
}

/** List, make and revoke the access tokens of a server (needs an admin token). */
export function ManageAccessDialog({ library, onClose }: { library: LibraryInfo; onClose: () => void }) {
  const [tokens, setTokens] = useState<RemoteToken[]>();
  const [name, setName] = useState("");
  const [role, setRole] = useState<RemoteRole>("read");
  const [made, setMade] = useState<{ name: string; role: RemoteRole; token: string }>();
  const [copied, setCopied] = useState(false);
  const [revoking, setRevoking] = useState<RemoteToken>();
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();

  const reload = useCallback(async () => {
    try { setTokens(await listRemoteTokens(library.id)); setError(undefined); }
    catch (failure) { setError(reason(failure)); }
  }, [library.id]);
  useEffect(() => { void reload(); }, [reload]);

  const create = async () => {
    setBusy("create"); setError(undefined);
    try {
      const created = await createRemoteToken(library.id, name.trim(), role);
      setMade({ name: created.name, role: created.role, token: created.token });
      setCopied(false); setName("");
      await reload();
    } catch (failure) { setError(reason(failure)); }
    finally { setBusy(undefined); }
  };
  const revoke = async (token: RemoteToken) => {
    setBusy(`revoke:${token.id}`); setError(undefined);
    try { await revokeRemoteToken(library.id, token.id); setRevoking(undefined); await reload(); }
    catch (failure) { setRevoking(undefined); setError(reason(failure)); }
    finally { setBusy(undefined); }
  };
  const copy = async () => {
    if (!made) return;
    try { await navigator.clipboard.writeText(made.token); setCopied(true); }
    catch { setError("The token could not be copied. Select it and copy it by hand."); }
  };

  return <>
    <Dialog open size="medium" title={`Access to "${library.name}"`} onClose={() => { if (!busy) onClose(); }} footer={<Button variant="primary" onClick={onClose}>Done</Button>}>
      <div className="remote-dialog" aria-busy={!!busy}>
        {made && <div className="remote-dialog__field" role="status">
          <p className="remote-dialog__ok">New token for {made.name} ({roleLabel(made.role)})</p>
          <div className="remote-dialog__secret">
            <TextInput readOnly aria-label="New token" value={made.token} onFocus={(event) => event.currentTarget.select()} />
            <Button onClick={() => void copy()}>{copied ? "Copied" : "Copy"}</Button>
          </div>
          <p className="remote-dialog__warning">{NEW_TOKEN_WARNING}</p>
          <Button variant="ghost" onClick={() => setMade(undefined)}>I have copied it</Button>
        </div>}
        <ul className="remote-tokens" aria-label="Access tokens">
          {(tokens ?? []).map((token) => <li key={token.id} className="remote-tokens__item">
            <div className="remote-tokens__main">
              <strong title={token.name}>{token.name}</strong>
              <small>{roleLabel(token.role)} · created {formatEpoch(token.createdAt)} · last used {formatEpoch(token.lastUsedAt)}</small>
            </div>
            <Button variant="ghost" disabled={!!busy} onClick={() => setRevoking(token)}>Revoke…</Button>
          </li>)}
          {tokens && !tokens.length && <li className="remote-tokens__item"><small>No tokens.</small></li>}
          {!tokens && !error && <li className="remote-tokens__item"><small>Loading…</small></li>}
        </ul>
        <div className="remote-new-token">
          <label className="remote-dialog__field"><span>New token for</span>
            <TextInput value={name} placeholder="e.g. Alice's laptop" maxLength={64} disabled={!!busy} onChange={(event) => setName(event.currentTarget.value)} />
          </label>
          <label className="remote-dialog__field"><span>Can</span>
            <Select value={role} options={TOKEN_ROLES.map((item) => ({ value: item.value, label: item.label }))} onChange={(value) => setRole(value as RemoteRole)} disabled={!!busy} />
          </label>
          <Button variant="primary" disabled={!!busy || !name.trim()} onClick={() => void create()}>{busy === "create" ? "Making…" : "New token"}</Button>
        </div>
        <p className="remote-dialog__note">{roleSummary(role)}</p>
        {error && <p className="remote-dialog__error" role="alert">{error}</p>}
      </div>
    </Dialog>
    {revoking && <ConfirmationDialog title={`Revoke "${revoking.name}"?`} onClose={() => setRevoking(undefined)}
      description="Whoever uses this token will be locked out at once."
      note="This cannot be undone. The server refuses to revoke its last admin token, so you cannot lock yourself out of managing access."
      confirmLabel="Revoke token" busyLabel="Revoking…" busy={busy === `revoke:${revoking.id}`} onConfirm={() => void revoke(revoking)} />}
  </>;
}
