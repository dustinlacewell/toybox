import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const url = await load("remoteUrl.ts");
const remote = await load("remoteLibrary.ts");
const transfer = await load("transfer.ts");

const ok = (input) => {
  const result = url.normalizeServerUrl(input);
  assert.equal(result.ok, true, `${input}: ${result.error}`);
  return result;
};

test("a server address gets https, no trailing slash and no api suffix", () => {
  assert.equal(ok("assets.example.com").url, "https://assets.example.com");
  assert.equal(ok("  Assets.Example.com/  ").url, "https://assets.example.com");
  assert.equal(ok("https://assets.example.com:8443/toybox/").url, "https://assets.example.com:8443/toybox");
  assert.equal(ok("https://assets.example.com/api/v1").url, "https://assets.example.com");
  assert.equal(ok("https://assets.example.com/?x=1#top").url, "https://assets.example.com");
  assert.equal(ok("assets.example.com:8787").url, "https://assets.example.com:8787");
  assert.equal(ok("HTTPS://a.example.com").warning, undefined);
  assert.equal(ok("https://[2001:db8::1]:8443").url, "https://[2001:db8::1]:8443");
});

test("plain http is accepted only for this computer and private networks, with a warning", () => {
  for (const address of ["http://localhost:8787", "http://127.0.0.1:8787/", "http://[::1]:8787", "http://127.5.5.5"]) {
    assert.equal(ok(address).warning, url.LOCAL_WARNING, address);
  }
  for (const address of ["http://192.168.1.20:8787", "http://10.0.0.5", "http://172.16.0.1", "http://172.31.255.254", "http://nas.local:8787"]) {
    assert.equal(ok(address).warning, url.LAN_WARNING, address);
  }
  for (const address of ["http://example.com", "http://8.8.8.8", "http://172.32.0.1", "http://172.15.0.1", "http://192.169.0.1", "http://local", "http://notlocal.localhost.com"]) {
    const result = url.normalizeServerUrl(address);
    assert.equal(result.ok, false, address);
    assert.match(result.error, /https:\/\//);
  }
  assert.equal(ok("https://192.168.1.20").warning, undefined, "https never warns");
  assert.equal(ok("localhost:8787").url, "https://localhost:8787", "no scheme means https, even for localhost");
});

test("nonsense addresses are refused with a sentence", () => {
  for (const bad of ["", "   ", "ftp://example.com", "https://", "https://user:pw@example.com", "https://exa mple.com", "https://example.com:0", "https://example.com:99999", "https://example.com:abc", "https://-bad.example.com", "https://a..b.com", "ws://example.com"]) {
    const result = url.normalizeServerUrl(bad);
    assert.equal(result.ok, false, bad);
    assert.match(result.error, /\.$/, bad);
  }
});

test("role and offline badges", () => {
  const remoteLibrary = (role, offline = false) => ({ kind: "remote", offline, remote: { role, serverName: "S", offline, url: "https://s" } });
  assert.deepEqual(remote.libraryBadges(remoteLibrary("read")), ["Remote · read-only"]);
  assert.deepEqual(remote.libraryBadges(remoteLibrary("write")), ["Remote · can edit"]);
  assert.deepEqual(remote.libraryBadges(remoteLibrary("admin", true)), ["Remote · admin", "Offline"]);
  assert.deepEqual(remote.libraryBadges({ kind: "local" }), []);
  assert.equal(remote.isRemote({ kind: "remote" }), true);
  assert.equal(remote.libraryLabel({ name: "Cloud", kind: "remote" }), "Cloud (remote)");
  assert.equal(remote.libraryLabel({ name: "Disk", kind: "local" }), "Disk");
});

test("the add dialog says what the token allows", () => {
  assert.equal(remote.connectedLine("My Toybox", "read"), "Connected to My Toybox as read-only");
  assert.equal(remote.connectedLine("My Toybox", "write"), "Connected to My Toybox as can edit");
  assert.equal(remote.connectedLine("", "admin"), "Connected to the server as admin");
  assert.match(remote.roleSummary("read"), /Cannot change anything/);
});

test("managing access needs an admin token and a reachable server", () => {
  const library = (role, offline = false) => ({ kind: "remote", remote: { role, offline, serverName: "", url: "" } });
  assert.equal(remote.canManageAccess(library("admin")), true);
  assert.equal(remote.canManageAccess(library("write")), false);
  assert.equal(remote.canManageAccess(library("admin", true)), false);
  assert.equal(remote.canManageAccess({ kind: "local" }), false);
  assert.match(remote.manageAccessReason(library("read")), /not an admin token/);
  assert.match(remote.manageAccessReason(library("admin", true)), /cannot be reached/);
  assert.equal(remote.manageAccessReason(library("admin")), null);
});

test("sizes, dates and progress read as plain text", () => {
  assert.equal(remote.formatBytes(0), "0 B");
  assert.equal(remote.formatBytes(1400), "1.4 KB");
  assert.equal(remote.formatBytes(2_100_000_000), "2.1 GB");
  assert.equal(remote.formatBytes(250_000), "250 KB");
  assert.equal(remote.formatEpoch(null), "never");
  assert.equal(remote.formatEpoch(0), "never");
  assert.match(remote.formatEpoch(1_700_000_000), /2023/);
  assert.equal(remote.progressLine({ operation: "upload", doneFiles: 3, totalFiles: 12, doneBytes: 45_000_000, totalBytes: 300_000_000 }), "Uploading 3 of 12 files (45.0 MB of 300 MB)…");
  assert.equal(remote.progressLine({ operation: "download", doneFiles: 5, totalFiles: 2, doneBytes: 0, totalBytes: 0 }), "Downloading 2 of 2 files…");
  assert.match(remote.NEW_TOKEN_WARNING, /only once/);
});

const lib = (id, extra = {}) => ({ id, name: id.toUpperCase(), kind: "local", root: `/${id}`, available: true, canWrite: true, canEditFiles: true, isDefault: false, ...extra });
const cloud = (id, role, extra = {}) => lib(id, {
  kind: "remote", canWrite: role !== "read", canEditFiles: false, offline: false,
  remote: { url: `https://${id}.example.com`, role, serverName: id, offline: false }, ...extra,
});

test("a library with a read token or no server cannot be written, and says why", () => {
  assert.equal(transfer.unwritableReason(cloud("a", "read")), "your access token can only read");
  assert.equal(transfer.unwritableReason(cloud("a", "write")), null);
  assert.equal(transfer.unwritableReason(cloud("a", "admin")), null);
  assert.equal(transfer.unwritableReason(cloud("a", "write", { canWrite: false, offline: true, remote: { url: "", role: "write", serverName: "", offline: true } })), "the server cannot be reached right now");
  assert.match(transfer.unwritableReason(cloud("a", "write", { available: false, canWrite: false })), /not been reachable yet/);
  // The local reasons are unchanged.
  assert.equal(transfer.unwritableReason(lib("a", { canWrite: false })), "it is read-only");
  assert.equal(transfer.unwritableReason(lib("a", { available: false, canWrite: false })), "its folder cannot be read right now");
});

test("transfer targets include writable remote libraries and block the others with the reason", () => {
  const libs = [lib("disk"), cloud("edit", "write"), cloud("view", "read")];
  const result = transfer.transferTargets(libs, ["disk"]);
  assert.deepEqual(result.targets.map((l) => l.id), ["edit"]);
  assert.deepEqual(result.blocked.map((b) => [b.library.id, b.reason]), [["view", "your access token can only read"]]);
  // Moving out of a library you can only read is not offered.
  assert.deepEqual(transfer.moveBlockers(libs, ["view"], "disk").map((b) => b.library.id), ["view"]);
  assert.deepEqual(transfer.moveBlockers(libs, ["edit"], "disk"), []);
});

test("files of a remote library are never edited in place, with a reason", () => {
  // Writable remote: deleting is fine, editing files is not.
  assert.equal(transfer.readOnlyNotice("delete", transfer.readOnlyOwners([cloud("edit", "admin")], [{ library: "edit" }])), undefined);
  const blockers = transfer.fileEditBlockers([cloud("edit", "admin"), lib("disk")], [{ library: "edit" }, { library: "disk" }]);
  assert.deepEqual(blockers.map((b) => b.library.id), ["edit"]);
  assert.match(blockers[0].reason, /cannot be edited in place yet/);
  assert.match(transfer.readOnlyNotice("change this model", blockers), /^Cannot change this model: EDIT \(files of a remote library cannot be edited in place yet/);
  // A read-only remote is explained by its token first.
  assert.equal(transfer.fileEditBlockers([cloud("view", "read")], [{ library: "view" }])[0].reason, "your access token can only read");
  // Local libraries are unaffected, and an older answer without canEditFiles follows canWrite.
  assert.deepEqual(transfer.fileEditBlockers([lib("disk")], [{ library: "disk" }]), []);
  assert.deepEqual(transfer.fileEditBlockers([lib("disk", { canEditFiles: undefined })], [{ library: "disk" }]), []);
  assert.equal(transfer.fileEditBlockers([lib("disk", { canWrite: false })], [{ library: "disk" }])[0].reason, "it is read-only");
  assert.deepEqual(transfer.fileEditBlockers([cloud("edit", "admin")], [{ library: "elsewhere" }]), [], "only the libraries the items belong to count");
});

test("a version sentence is shown for a notice or a refusal and not when the versions fit", () => {
  const library = (compat, compatMessage) => ({ kind: "remote", remote: { role: "read", serverName: "S", offline: false, url: "https://s", compat, compatMessage } });
  assert.equal(remote.versionNotice(library("ok", undefined)), undefined);
  assert.equal(remote.versionNotice(library(undefined, undefined)), undefined);
  assert.equal(remote.versionNotice({ kind: "local" }), undefined);
  assert.equal(remote.versionNotice(library("notice", "This server runs Toybox 0.4.0.")), "This server runs Toybox 0.4.0.");
  assert.equal(remote.versionNotice(library("client_too_old", "Update Toybox to 0.5.0 or newer.")), "Update Toybox to 0.5.0 or newer.");
});
