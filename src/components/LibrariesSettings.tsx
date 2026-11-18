/**
 * The Library page of Settings: one compact row per registered library (name,
 * folder or host, a few tags, the item count) and a settings button that opens
 * the rest: name, location, which library receives imports, the remote access
 * actions, and rescan / remove. Removing only unregisters; the folder and the
 * server are never touched.
 */

import { useEffect, useRef, useState } from "react";
import { Button, ConfirmationDialog, TextInput } from "@ldlework/toybox-sdk/ui";
import { Folder, Plus, SlidersHorizontal } from "lucide-react";

import type { LibraryInfo } from "../domain/catalog";
import { itemCount, libraryTags, librarySecondary, shortenPath } from "../domain/librarySettings";
import { unwritableReason } from "../domain/transfer";
import { forgetLibrary, refreshLibraryList, syncLibrary } from "../services/libraries";
import { pickDirectory } from "../services/pickDirectory";
import { addLibrary, createLibrary, removeLibrary, renameLibrary, setDefaultLibrary } from "../services/tauriApi";
import { useStore } from "../state/store";
import { AddRemoteLibraryDialog } from "./AddRemoteLibraryDialog";
import { RemoteLibraryDetails } from "./RemoteLibraryDetails";
import { RemoteMark } from "./RemoteMark";

export function LibrariesSettings() {
  const libraries = useStore((state) => state.libraries);
  const errors = useStore((state) => state.libraryErrors);
  const fingerprinting = useStore((state) => state.fingerprinting);
  const [busy, setBusy] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const [removing, setRemoving] = useState<LibraryInfo>();
  const [addingRemote, setAddingRemote] = useState(false);
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const [menuOpen, setMenuOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  // How many items each library holds (as it said when it was last made ready).
  const counts = useStore((state) => state.libraryCounts);

  // The add menu closes on Escape or a click elsewhere.
  useEffect(() => {
    if (!menuOpen) return;
    const away = (event: MouseEvent) => { if (!menu.current?.contains(event.target as Node)) setMenuOpen(false); };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation(); setMenuOpen(false);
      menu.current?.querySelector<HTMLButtonElement>("button")?.focus();
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", escape, true);
    return () => { document.removeEventListener("mousedown", away); document.removeEventListener("keydown", escape, true); };
  }, [menuOpen]);

  const toggle = (id: string) => setOpen((current) => { const next = new Set(current); if (!next.delete(id)) next.add(id); return next; });
  const run = async (key: string, work: () => Promise<void>) => {
    setBusy(key); setFailure(undefined);
    try { await work(); } catch (e) { setFailure(String(e).replace(/^Error: /, "")); } finally { setBusy(undefined); }
  };
  const addFrom = (create: boolean) => run(create ? "create" : "add", async () => {
    const folder = await pickDirectory();
    if (!folder) return;
    const added = await (create ? createLibrary(folder) : addLibrary(folder));
    await syncLibrary(added.id, false);
  });
  const choose = (action: () => void) => { setMenuOpen(false); action(); };

  return <>
    <h2>Libraries</h2>
    <p className="settings__description">The folders and servers whose items Toybox shows together.</p>
    <ul className="libraries" aria-label="Libraries">
      {libraries.map((library) => {
        const expanded = open.has(library.id);
        const panel = `library-${library.id}`;
        const tags = libraryTags(library);
        const secondary = librarySecondary(library);
        const reason = unwritableReason(library);
        const error = errors[library.id] ?? (!library.available && !library.remote ? "This folder cannot be read right now." : undefined);
        const rescanKey = `scan:${library.id}`;
        return <li key={library.id} className={`libraries__item${expanded ? " is-open" : ""}`}>
          <div className="libraries__row" onClick={(event) => { if (!(event.target as HTMLElement).closest("button")) toggle(library.id); }}>
            {library.remote ? <RemoteMark library={library} /> : <span className="libraries__kind" title="Folder on this computer"><Folder size={14} aria-hidden="true" /></span>}
            <span className="libraries__title">{library.name}</span>
            {tags.map((tag) => <span key={tag.text} className={`libraries__tag${tag.tone === "warning" ? " libraries__tag--warning" : ""}`}>{tag.text}</span>)}
            <span className="libraries__secondary" title={secondary}>{library.remote ? secondary : shortenPath(secondary)}</span>
            <span className="libraries__count">{itemCount(counts[library.id] ?? 0)}</span>
            <button type="button" className="libraries__toggle" aria-expanded={expanded} aria-controls={panel} aria-label={`Settings for ${library.name}`} title="Settings" onClick={() => toggle(library.id)}>
              <SlidersHorizontal size={14} aria-hidden="true" />
            </button>
          </div>
          {expanded && <div className="libraries__panel" id={panel}>
            <dl className="libraries__details">
              <dt><label htmlFor={`${panel}-name`}>Name</label></dt>
              <dd><TextInput key={library.name} id={`${panel}-name`} className="libraries__name" defaultValue={library.name} disabled={!!busy}
                onBlur={(event) => { const name = event.currentTarget.value.trim(); if (name && name !== library.name) void run(`rename:${library.id}`, async () => { await renameLibrary(library.id, name); await refreshLibraryList(); }); else event.currentTarget.value = library.name; }}
                onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} /></dd>
              <dt>Location</dt>
              <dd className="libraries__location">{library.remote ? <>{library.remote.url}{library.remote.serverName ? ` · ${library.remote.serverName}` : ""}</> : library.root}</dd>
              <dt>New imports</dt>
              <dd><label className="libraries__default"><input type="checkbox" checked={library.isDefault} disabled={!!busy || (!library.isDefault && !!reason)}
                onChange={(event) => { if (event.currentTarget.checked) void run(`default:${library.id}`, async () => { await setDefaultLibrary(library.id); await refreshLibraryList(); }); }} />
                {library.isDefault ? "This is where new imports go" : "Use for new imports"}</label>
                {!library.isDefault && reason && <span className="libraries__why"> Not available because {reason}.</span>}</dd>
              {library.remote && <RemoteLibraryDetails library={library} busy={busy} run={run} onChanged={() => syncLibrary(library.id, false)} />}
            </dl>
            {error && <p className="settings__error" role="alert">{error}</p>}
            {fingerprinting[library.id] && <p className="libraries__fingerprint" role="status">
              Fingerprinting items for duplicate detection… {fingerprinting[library.id].done.toLocaleString()} of {fingerprinting[library.id].total.toLocaleString()}</p>}
            <div className="libraries__actions">
              <Button variant="ghost" disabled={!!busy} onClick={() => void run(rescanKey, () => syncLibrary(library.id, true))}>
                {busy === rescanKey ? (library.remote ? "Refreshing…" : "Scanning…") : library.remote ? "Refresh from server" : "Rescan"}</Button>
              <Button variant="ghost" className="libraries__remove" disabled={!!busy} onClick={() => setRemoving(library)}>Remove…</Button>
            </div>
          </div>}
        </li>;
      })}
    </ul>
    {!libraries.length && <p className="settings__description">No library is set up yet. Add a folder or a server to begin.</p>}
    {failure && <p className="settings__error" role="alert">{failure}</p>}
    <div className="libraries__add" ref={menu}>
      <Button disabled={!!busy} aria-haspopup="true" aria-expanded={menuOpen} aria-controls="library-add-menu" onClick={() => setMenuOpen((value) => !value)}>
        <Plus size={14} aria-hidden="true" /> {busy === "add" ? "Adding…" : busy === "create" ? "Creating…" : "Add library…"}</Button>
      {menuOpen && <div className="libraries__menu" id="library-add-menu" role="group" aria-label="Add a library">
        <button type="button" autoFocus onClick={() => choose(() => void addFrom(false))}>Existing folder…</button>
        <button type="button" onClick={() => choose(() => void addFrom(true))}>New folder…</button>
        <button type="button" onClick={() => choose(() => setAddingRemote(true))}>Remote server…</button>
      </div>}
    </div>
    {addingRemote && <AddRemoteLibraryDialog onAdded={(added) => syncLibrary(added.id, false)} onClose={() => setAddingRemote(false)} />}
    {removing && <ConfirmationDialog title={`Remove "${removing.name}"?`} onClose={() => setRemoving(undefined)}
      description="Toybox will stop showing this library."
      note={removing.remote
        ? <>The server and everything on it are left exactly as they are. The access token is removed from this computer's credential store and the files downloaded from it are deleted. Your favourites, tags and collections for it are kept.<br/><br/>{removing.remote.url}</>
        : <>The folder and all of its files are left exactly as they are, and nothing is deleted. Your favourites, tags and collections for it are kept, so adding the same folder again brings them back.<br/><br/>{removing.root}</>}
      confirmLabel="Remove library" busyLabel="Removing…" busy={busy === `remove:${removing.id}`} onConfirm={() => void run(`remove:${removing.id}`, async () => {
        await removeLibrary(removing.id); await forgetLibrary(removing.id); setRemoving(undefined);
      })} />}
  </>;
}
