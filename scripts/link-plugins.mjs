/**
 * Dev convenience: link each in-repo plugin under `plugins/<id>/` into the app's
 * runtime plugins dir (`%APPDATA%\com.toybox.app\plugins\<id>`) so the running
 * app discovers it live. Idempotent — re-running refreshes the links.
 *
 * On Windows this creates a directory junction (no admin needed); elsewhere a
 * symlink. Run after building a plugin:
 *
 *   node plugins/com.toybox.placer  (build) ; node scripts/link-plugins.mjs
 *
 * (A future `wm plugins:link` command is the proper home for this once the
 * workspace adopts workmark.)
 */

import { mkdirSync, readdirSync, rmSync, statSync, lstatSync, readlinkSync, unlinkSync, rmdirSync, symlinkSync, existsSync } from "node:fs";
import { join, resolve, relative, isAbsolute } from "node:path";
import { platform, homedir } from "node:os";

const APP_IDENTIFIER = "com.toybox.app";

function appDataPluginsDir() {
  if (platform() === "win32") {
    const base = process.env.APPDATA ?? join(homedir(), "AppData", "Roaming");
    return join(base, APP_IDENTIFIER, "plugins");
  }
  if (platform() === "darwin") {
    return join(homedir(), "Library", "Application Support", APP_IDENTIFIER, "plugins");
  }
  const base = process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share");
  return join(base, APP_IDENTIFIER, "plugins");
}

const repoPlugins = new URL("../plugins/", import.meta.url);
const repoPluginsDir = repoPlugins.pathname.replace(/^\/([A-Za-z]:)/, "$1");

const target = appDataPluginsDir();
mkdirSync(target, { recursive: true });

const linkType = platform() === "win32" ? "junction" : "dir";
let linked = 0;

for (const id of readdirSync(repoPluginsDir)) {
  const src = join(repoPluginsDir, id);
  if (!statSync(src).isDirectory()) continue;
  const dst = join(target, id);
  if (existsSync(dst)) rmSync(dst, { recursive: true, force: true });
  symlinkSync(src, dst, linkType);
  console.log(`linked ${id} -> ${dst}`);
  linked += 1;
}

// Remove links left behind by a plugin folder that was renamed or deleted in the
// repo. Only links (junctions or symlinks) into the repo plugins dir whose target
// is gone are removed; a real folder, or a link to anywhere else, stays.
let removed = 0;
for (const name of readdirSync(target)) {
  const dst = join(target, name);
  let link;
  try {
    if (!lstatSync(dst).isSymbolicLink()) continue;
    link = resolve(target, readlinkSync(dst));
  } catch {
    continue;
  }
  const inside = relative(repoPluginsDir, link);
  if (inside === "" || inside.startsWith("..") || isAbsolute(inside) || existsSync(link)) continue;
  try {
    unlinkSync(dst);
  } catch {
    rmdirSync(dst);
  }
  console.log(`removed stale link ${name}`);
  removed += 1;
}

console.log(`\n${linked} plugin(s) linked into ${target}${removed ? `, ${removed} stale link(s) removed` : ""}`);
