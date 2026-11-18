/**
 * Presenting remote libraries: the role badge, the offline badge, what a role
 * allows, and the small formatting helpers the access dialogs use. Pure: no
 * imports at run time, so the tests load this file on its own.
 */

export type RemoteRole = "read" | "write" | "admin";

/** The parts of `LibraryInfo` this module reads (the full type is in `catalog.ts`). */
export interface RemoteFacts {
  kind: string;
  available?: boolean;
  offline?: boolean;
  remote?: { role: RemoteRole; serverName: string; offline: boolean; url: string };
}

export const isRemote = (library: Pick<RemoteFacts, "kind">): boolean => library.kind === "remote";

/** A library's name as a list option shows it: remote ones say so (a select has no room for an icon). */
export function libraryLabel(library: { name: string; kind: string }): string {
  return library.kind === "remote" ? `${library.name} (remote)` : library.name;
}

/** "Remote · read-only", "Remote · can edit", "Remote · admin". */
export function roleBadge(role: RemoteRole): string {
  return role === "read" ? "Remote · read-only" : role === "write" ? "Remote · can edit" : "Remote · admin";
}

/** The badges a library row shows: none for a local library. */
export function libraryBadges(library: RemoteFacts): string[] {
  if (!library.remote) return [];
  const badges = [roleBadge(library.remote.role)];
  if (library.remote.offline || library.offline) badges.push("Offline");
  return badges;
}

/** What a role means, in words for the dialogs. */
export function roleSummary(role: RemoteRole): string {
  return role === "read"
    ? "Can browse and download. Cannot change anything."
    : role === "write"
      ? "Can also import, delete and copy items in."
      : "Can also manage access tokens.";
}

/** The role names in the token form, weakest first. */
export const TOKEN_ROLES: { value: RemoteRole; label: string }[] = [
  { value: "read", label: "Read-only" },
  { value: "write", label: "Can edit" },
  { value: "admin", label: "Admin" },
];

export const roleLabel = (role: RemoteRole): string => TOKEN_ROLES.find((item) => item.value === role)?.label ?? role;

/** "Connected to <server> as <role>" for the add dialog. */
export function connectedLine(serverName: string, role: RemoteRole): string {
  const who = role === "read" ? "read-only" : role === "write" ? "can edit" : "admin";
  return `Connected to ${serverName || "the server"} as ${who}`;
}

/** Managing access needs the admin role and a reachable server. */
export function canManageAccess(library: RemoteFacts): boolean {
  return !!library.remote && library.remote.role === "admin" && !library.remote.offline;
}

/** Why access cannot be managed, or null when it can. */
export function manageAccessReason(library: RemoteFacts): string | null {
  if (!library.remote) return "this library is on this computer";
  if (library.remote.role !== "admin") return "your access token is not an admin token";
  if (library.remote.offline) return "the server cannot be reached right now";
  return null;
}

export const NEW_TOKEN_WARNING = "Copy this token now. It is shown only once and cannot be looked up again. Anyone who has it can use this library with the role you chose.";

/** A byte count for people: 0 B, 1.4 MB, 2.1 GB. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) { value /= 1000; unit += 1; }
  return `${unit === 0 || value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

/** Seconds since the Unix epoch as a short local date, or a dash. */
export function formatEpoch(seconds: number | null | undefined): string {
  if (!seconds) return "never";
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? "never" : date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/** A progress line for an upload or download in progress. */
export function progressLine(progress: { operation: string; doneFiles: number; totalFiles: number; doneBytes: number; totalBytes: number }): string {
  const verb = progress.operation === "upload" ? "Uploading" : "Downloading";
  const files = `${Math.min(progress.doneFiles, progress.totalFiles)} of ${progress.totalFiles} files`;
  return progress.totalBytes > 0 ? `${verb} ${files} (${formatBytes(progress.doneBytes)} of ${formatBytes(progress.totalBytes)})…` : `${verb} ${files}…`;
}
