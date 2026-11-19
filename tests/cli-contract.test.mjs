// The command-line contract of the plugins (docs/cli.md, "Plugins and the command
// line"): every manifest's `cli` section is valid and agrees with its module, and
// every `cli.module` can run in the engine `tbx` embeds. Run the plugin builds
// (`tsc -p plugins/<id>/tsconfig.json`, cores first) before this file.

import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import ts from "typescript";

import { loadPlugins, manifestOf, pluginDir, pluginIds, repo, tempDir } from "./support/cli-host.mjs";
import { validateCli } from "./support/cli-manifest.mjs";

const withCli = pluginIds().filter((id) => manifestOf(id).cli);
const SDK = "@ldlework/toybox-sdk";

test("the plugins that add to the command line are the expected ones", () => {
  assert.deepEqual(withCli, [
    "com.toybox.animation-core", "com.toybox.audio-core", "com.toybox.material-core",
    "com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.placer", "com.toybox.skybox-core",
  ]);
  const importers = withCli.flatMap((id) => (manifestOf(id).cli.importers ?? []).map((importer) => importer.name)).sort();
  assert.deepEqual(importers, ["animation", "audio", "material", "model", "skybox"]);
  assert.equal(manifestOf("com.toybox.placer").cli.exporter.name, "placer");
});

// --- manifests -------------------------------------------------------------------

test("every manifest's cli section follows the rules of docs/cli.md and agrees with its module", async () => {
  const host = await loadPlugins(withCli, { libraryRoot: tempDir("library") });
  for (const id of withCli) {
    const { manifest, module } = host.plugins.get(id);
    assert.ok(existsSync(path.join(pluginDir(id), manifest.cli.module)), `${id}: ${manifest.cli.module} is built`);
    assert.deepEqual(validateCli(manifest, module), [], id);
  }
});

test("a plugin whose entry has no user interface names it as its cli module; the others have a file of their own", () => {
  for (const id of ["com.toybox.model-core", "com.toybox.model-gltf", "com.toybox.placer"]) assert.equal(manifestOf(id).cli.module, manifestOf(id).entry, id);
  for (const id of ["com.toybox.audio-core", "com.toybox.material-core", "com.toybox.animation-core", "com.toybox.skybox-core"]) {
    assert.equal(manifestOf(id).cli.module, "dist/cli.js", id);
    assert.notEqual(manifestOf(id).cli.module, manifestOf(id).entry, id);
  }
});

test("a format plugin's cli module contributes its formats, so the models importer sees them", async () => {
  const host = await loadPlugins(["com.toybox.model-core", "com.toybox.model-gltf"], { libraryRoot: tempDir("library") });
  const formats = host.lookup("model.formats");
  assert.deepEqual(formats.map((format) => [format.pluginId, format.value.id]), [["com.toybox.model-gltf", "gltf"]]);
  assert.equal(host.lookup("model.api").length, 1);
});

test("the validator catches what the rules forbid", () => {
  const base = { id: "x", cli: { module: "dist/cli.js" } };
  const bad = (cli, module, manifest = {}) => validateCli({ ...base, ...manifest, cli: { ...base.cli, ...cli } }, module);
  const has = (problems, pattern) => assert.ok(problems.some((problem) => pattern.test(problem)), `${pattern} in ${JSON.stringify(problems)}`);

  assert.deepEqual(bad({ importers: [{ name: "ok", summary: "s", options: [{ name: "a-b", type: "string" }] }] }, { cli: { importers: { ok: async () => {} } } }), []);
  has(bad({ importers: [{ name: "Bad", summary: "s" }] }), /name must match/);
  has(bad({ importers: [{ name: "a", summary: "s" }, { name: "a", summary: "s" }] }), /used twice/);
  has(bad({ importers: [{ name: "2fast", summary: "s" }] }), /name must match/);
  has(bad({ commands: [{ name: "g", summary: "s", commands: [{ name: "c", summary: "s" }], args: [{ name: "f", type: "path" }] }] }), /not both/);
  has(bad({ commands: [{ name: "g", summary: "s", commands: [] }] }), /group needs commands/);
  has(bad({ commands: [{ name: "c", summary: "s", options: [{ name: "Under_score", type: "string" }] }] }), /kebab-case/);
  has(bad({ commands: [{ name: "c", summary: "s", args: [{ name: "f", type: "flag" }] }] }), /cannot be a flag/);
  has(bad({ commands: [{ name: "c", summary: "s", options: [{ name: "f", type: "wat" }] }] }), /unknown type/);
  has(bad({ commands: [{ name: "c", summary: "s", options: [{ name: "f", type: "choice" }] }] }), /needs choices/);
  has(bad({ commands: [{ name: "c", summary: "s", options: [{ name: "f", type: "choice", choices: ["a"], default: "b" }] }] }), /does not fit/);
  has(bad({ commands: [{ name: "c", summary: "s", options: [{ name: "a-b", type: "string" }, { name: "a-b", type: "number" }] }] }), /used twice/);
  has(bad({ exporter: { name: "e", summary: "s" } }), /no options, and no manifest fields/);
  assert.deepEqual(bad({ exporter: { name: "e", summary: "s" } }, { run: async () => {} }, { fields: [{ key: "k", type: "text", label: "K" }] }), []);

  // The manifest and the module must agree both ways.
  const spec = { importers: [{ name: "a", summary: "s" }], commands: [{ name: "g", summary: "s", commands: [{ name: "c", summary: "s" }] }], exporter: { name: "e", summary: "s", options: [] } };
  has(bad(spec, {}), /importer a: the module has no handler/);
  has(bad(spec, {}), /command g c: the module has no handler/);
  has(bad(spec, {}), /exporter e: the module has no run/);
  has(bad({}, { cli: { importers: { stray: async () => {} } } }), /importer stray: the module handles it/);
  has(bad({}, { cli: { commands: { "g c": async () => {} } } }), /command g c: the module handles it/);
  assert.deepEqual(bad(spec, { run: async () => {}, cli: { importers: { a: async () => {} }, commands: { "g c": async () => {} } } }), []);
});

// --- module graphs ---------------------------------------------------------------

/** What a source file imports, and the first use of a name the engine does not have. */
function inspect(file) {
  const source = ts.createSourceFile(file, readFileSync(file, "utf8"), ts.ScriptTarget.ES2022, true, ts.ScriptKind.JS);
  const imports = [];
  const uses = [];
  const FORBIDDEN = new Set(["document", "window", "fetch", "Blob"]);
  const GUARDED = new Set(["navigator"]); // absent in tbx; allowed behind a typeof test
  const visit = (node) => {
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) imports.push(node.moduleSpecifier.text);
    else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword && ts.isStringLiteralLike(node.arguments[0])) imports.push(node.arguments[0].text);
    else if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "require") imports.push(`require(${node.arguments[0]?.getText(source)})`);
    else if (ts.isIdentifier(node)) {
      const parent = node.parent;
      const isName = (ts.isPropertyAccessExpression(parent) && parent.name === node) || (ts.isPropertyAssignment(parent) && parent.name === node)
        || (ts.isMethodDeclaration(parent) && parent.name === node) || (ts.isPropertyDeclaration(parent) && parent.name === node)
        || (ts.isBindingElement(parent) && parent.propertyName === node) || (ts.isShorthandPropertyAssignment(parent) && false);
      const declared = (ts.isVariableDeclaration(parent) || ts.isParameter(parent) || ts.isFunctionDeclaration(parent) || ts.isImportSpecifier(parent)) && parent.name === node;
      if (!isName && !declared) {
        if (FORBIDDEN.has(node.text)) uses.push(node.text);
        else if (GUARDED.has(node.text) && !ts.isTypeOfExpression(parent)) {
          // `typeof navigator === "undefined" ? undefined : navigator` is a guard; so is a name reached through a variable guarded that way.
          const guarded = ts.isConditionalExpression(parent) || ts.isConditionalExpression(parent.parent);
          if (!guarded) uses.push(node.text);
        }
      }
    }
    if (ts.isPropertyAccessExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === "URL" && node.name.text === "createObjectURL") uses.push("URL.createObjectURL");
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { imports, uses };
}

/** Everything a module reaches by relative imports, with the bare imports and forbidden uses found on the way. */
function walkGraph(entry) {
  const seen = new Map();
  const bare = [];
  const uses = [];
  const queue = [entry];
  while (queue.length) {
    const file = queue.shift();
    if (seen.has(file)) continue;
    assert.ok(existsSync(file), `${path.relative(repo, file)} exists (build the plugin first)`);
    const found = inspect(file);
    seen.set(file, found);
    for (const use of found.uses) uses.push(`${path.relative(repo, file)} uses ${use}`);
    for (const specifier of found.imports) {
      if (specifier.startsWith(".")) queue.push(path.resolve(path.dirname(file), specifier));
      else bare.push({ file: path.relative(repo, file), specifier });
    }
  }
  return { files: [...seen.keys()], bare, uses };
}

test("no cli module reaches an import other than the SDK, or anything the engine does not have", () => {
  for (const id of withCli) {
    const { files, bare, uses } = walkGraph(path.join(pluginDir(id), manifestOf(id).cli.module));
    assert.ok(files.length >= 1);
    assert.deepEqual(bare.filter((item) => item.specifier !== SDK), [], `${id}: bare imports`);
    assert.deepEqual(uses, [], `${id}: window-only names`);
  }
});

test("the SDK entry the modules import has no imports of its own beyond its files, and needs no window", () => {
  const entry = path.join(repo, "packages", "toybox-sdk", "dist", "index.js");
  const { files, bare, uses } = walkGraph(entry);
  assert.deepEqual(bare, []);
  assert.deepEqual(uses, []);
  assert.ok(files.length >= 4);
});

test("the graph walk sees what it should: the skybox entry (which draws) is out of reach of its cli module, and its three.js use is caught", () => {
  const sky = path.join(pluginDir("com.toybox.skybox-core"), "dist");
  const cli = walkGraph(path.join(sky, "cli.js")).files.map((file) => path.relative(sky, file).split(path.sep).join("/"));
  assert.ok(cli.includes("lib/import.js") && cli.includes("lib/detect.js"));
  assert.ok(!cli.some((file) => file.startsWith("ui/")) && !cli.includes("lib/sky.js") && !cli.includes("index.js"));
  const entry = walkGraph(path.join(sky, "index.js"));
  assert.ok(entry.bare.some((item) => item.specifier === "three/addons/loaders/EXRLoader.js" || item.specifier === "react" || item.specifier === "three" || item.specifier.startsWith("@ldlework/toybox-sdk/")), "the entry module reaches bare imports the engine could not serve");
  // The audio panel is the other kind of offender: it uses Blob and URL.createObjectURL.
  const panel = walkGraph(path.join(pluginDir("com.toybox.audio-core"), "dist", "ui", "Import.js"));
  assert.ok(panel.uses.some((use) => /Blob|createObjectURL/.test(use)));
  assert.ok(panel.bare.some((item) => item.specifier === "react"));
});
