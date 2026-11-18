/**
 * What the user types as a server address becomes one normalised base URL. The
 * same rules as `toybox-remote`'s `url.rs` (which checks again when the library
 * is added), here so the dialog can say what is wrong as the user types.
 *
 * - a missing scheme means `https://`;
 * - `https://` is always fine; plain `http://` only for this computer
 *   (`localhost`, `127.0.0.1`, `[::1]`) and private networks (`10.x`,
 *   `172.16-31.x`, `192.168.x`, `*.local`), with a warning, because the access
 *   token is sent in every request;
 * - no trailing slash, no `/api/v1` suffix, no query or fragment, no password.
 *
 * Pure: no imports, so the tests load this file on its own.
 */

export type ServerAddress = { ok: true; url: string; warning?: string } | { ok: false; error: string };

export const LOCAL_WARNING = "This address is not encrypted. That is fine on this computer, but anything that is not on it should use https://.";
export const LAN_WARNING = "This address is not encrypted. Anyone on your network could read your access token. Use it only on a network you trust.";

type Reach = "this-computer" | "private-network" | "public";

function ipv4(host: string): number[] | null {
  const parts = host.split(".");
  if (parts.length !== 4) return null;
  const octets: number[] = [];
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part)) return null;
    const value = Number(part);
    if (value > 255) return null;
    octets.push(value);
  }
  return octets;
}

function reach(host: string): Reach {
  const name = host.toLowerCase();
  if (name === "localhost" || name === "127.0.0.1" || name === "[::1]") return "this-computer";
  const octets = ipv4(name);
  if (octets) {
    if (octets[0] === 127) return "this-computer";
    if (octets[0] === 10 || (octets[0] === 192 && octets[1] === 168)) return "private-network";
    if (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31) return "private-network";
    return "public";
  }
  if (name.endsWith(".local") && name.length > ".local".length) return "private-network";
  return "public";
}

function validHost(host: string): boolean {
  if (host.startsWith("[") && host.endsWith("]")) return /^\[[0-9a-fA-F:.]+\]$/.test(host);
  return host.length > 0 && host.length <= 253 && !/^[.-]|[.-]$/.test(host) && !host.includes("..") && /^[A-Za-z0-9.-]+$/.test(host);
}

/** `host` and `:port` of an authority; an IPv6 literal keeps its brackets. */
function splitAuthority(authority: string): [string, string | null] {
  if (authority.startsWith("[")) {
    const end = authority.indexOf("]");
    if (end < 0) return [authority, null];
    const rest = authority.slice(end + 1);
    if (rest === "") return [authority, null];
    return rest.startsWith(":") ? [authority.slice(0, end + 1), rest.slice(1)] : [authority, null];
  }
  const colon = authority.lastIndexOf(":");
  return colon < 0 ? [authority, null] : [authority.slice(0, colon), authority.slice(colon + 1)];
}

const fail = (error: string): ServerAddress => ({ ok: false, error });

export function normalizeServerUrl(input: string): ServerAddress {
  const text = input.trim();
  if (!text) return fail("Enter the address of the server.");
  if (/[\s\u0000-\u001f]/.test(text)) return fail("The address cannot contain spaces.");
  const marker = text.indexOf("://");
  const scheme = marker < 0 ? "https" : text.slice(0, marker).toLowerCase();
  const rest = marker < 0 ? text : text.slice(marker + 3);
  if (scheme !== "http" && scheme !== "https") {
    return fail("The address must start with https:// (or http:// for a server on this computer or your own network).");
  }
  const end = rest.search(/[/?#]/);
  const authority = end < 0 ? rest : rest.slice(0, end);
  const tail = end < 0 ? "" : rest.slice(end);
  if (authority.includes("@")) return fail("The address cannot contain a user name or password. Enter the access token separately.");
  const [host, port] = splitAuthority(authority);
  if (!validHost(host)) return fail("That does not look like a server address.");
  if (port !== null) {
    const number = /^\d+$/.test(port) ? Number(port) : NaN;
    if (!(number >= 1 && number <= 65535)) return fail("The port must be a number between 1 and 65535.");
  }
  let path = tail.split(/[?#]/)[0].replace(/\/+$/, "");
  for (const suffix of ["/api/v1", "/api"]) {
    if (path.endsWith(suffix)) { path = path.slice(0, -suffix.length); break; }
  }
  let warning: string | undefined;
  if (scheme === "http") {
    const where = reach(host);
    if (where === "public") return fail("Use an https:// address. Plain http:// would send your access token across the internet where anyone can read it.");
    warning = where === "this-computer" ? LOCAL_WARNING : LAN_WARNING;
  }
  const url = `${scheme}://${host.toLowerCase()}${port === null ? "" : `:${port}`}${path}`;
  return warning ? { ok: true, url, warning } : { ok: true, url };
}
