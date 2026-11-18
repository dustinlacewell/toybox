import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const { outputText } = ts.transpileModule(readFileSync(new URL("../src/domain/librarySettings.ts", import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
const m = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);

test("a long path keeps its start and end and stays within the limit", () => {
  const path = "D:\\dev\\resources\\very\\deep\\folder\\structure\\Toybox Library";
  const short = m.shortenPath(path, 30);
  assert.equal(short.length, 30);
  assert.ok(short.includes("…") && short.startsWith("D:\\") && short.endsWith("Library"));
  assert.equal(m.shortenPath("D:\\lib", 30), "D:\\lib");
});

test("the second line is the folder or the host", () => {
  assert.equal(m.librarySecondary({ kind: "local", root: "D:\\lib" }), "D:\\lib");
  assert.equal(m.librarySecondary({ kind: "remote", root: "C:\cache", remote: { role: "read", url: "https://assets.example.com:8443/toybox" } }), "assets.example.com:8443");
  assert.equal(m.hostOf("not a url"), "not a url");
});

test("tags appear only when they apply", () => {
  const texts = (l) => m.libraryTags(l).map((t) => t.text);
  assert.deepEqual(texts({ kind: "local", root: "x", available: true }), []);
  assert.deepEqual(texts({ kind: "local", root: "x", available: true, isDefault: true }), ["Default"]);
  assert.deepEqual(texts({ kind: "local", root: "x", available: false }), ["unavailable"]);
  assert.deepEqual(texts({ kind: "remote", root: "x", remote: { role: "write", url: "https://a.b" } }), ["can edit"]);
  assert.deepEqual(texts({ kind: "remote", root: "x", offline: true, isDefault: true, remote: { role: "admin", url: "https://a.b" } }), ["Default", "admin", "offline"]);
  assert.equal(m.libraryTags({ kind: "remote", root: "x", remote: { role: "read", url: "u", offline: true } }).at(-1).tone, "warning");
});

test("counts and roles read as words", () => {
  assert.equal(m.itemCount(1), "1 item");
  assert.equal(m.itemCount(0), "0 items");
  assert.match(m.accessSentence("read"), /^Read-only/);
  assert.equal(m.roleTag("read"), "read-only");
});
