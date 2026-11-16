import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, dirname, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import ts from "typescript";

const require = createRequire(import.meta.url);
const root = resolve("src/ds");
function files(dir) { return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => entry.isDirectory() ? files(resolve(dir, entry.name)) : [resolve(dir, entry.name)]); }

test("the design system imports only its own modules and presentation dependencies", () => {
  for (const file of files(root).filter((file) => /\.tsx?$/.test(file))) {
    const source = readFileSync(file, "utf8");
    for (const match of source.matchAll(/(?:from\s+|import\s*)["']([^"']+)["']/g)) {
      const dependency = match[1];
      if (dependency.startsWith(".")) assert.ok(resolve(dirname(file), dependency).startsWith(root + sep), `${file} crosses the design-system boundary: ${dependency}`);
      else assert.match(dependency, /^(react(?:-dom)?(?:\/.*)?|lucide-react)$/, `${file} imports an application dependency: ${dependency}`);
    }
  }
});

async function controlModule(name) {
  const source = readFileSync(resolve(root, name), "utf8");
  let { outputText } = ts.transpileModule(source, { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  outputText = outputText.replace(/from "(react(?:\/[^"\s]+)?)"/g, (_, dependency) => `from ${JSON.stringify(pathToFileURL(require.resolve(dependency)).href)}`);
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
}
function find(element, type) {
  if (!element || typeof element !== "object") return undefined;
  if (element.type === type) return element;
  return [element.props?.children].flat(Infinity).map((child) => find(child, type)).find(Boolean);
}

test("a tile reports selection changes and waits for its owner to update the value", async () => {
  const { PreviewTile } = await controlModule("PreviewTile.tsx");
  const changes = [];
  const props = { title: "Example", preview: null, active: false, selected: false, onPress() {}, onOpen() {}, onSelectedChange: (value) => changes.push(value) };
  const tile = PreviewTile(props);
  const checkbox = find(tile, "input");
  checkbox.props.onChange({ currentTarget: { checked: true } });
  assert.deepEqual(changes, [true]);
  assert.equal(checkbox.props.checked, false);
  assert.equal(find(PreviewTile({ ...props, selected: true }), "input").props.checked, true);
});

test("navigation and strips emit choices without owning application state", async () => {
  const { NavigationItem } = await controlModule("Workbench.tsx");
  const { ThumbnailStrip } = await controlModule("PreviewTile.tsx");
  let presses = 0;
  const item = NavigationItem({ label: "Example", active: false, onPress: () => presses++ });
  item.props.onClick();
  assert.equal(presses, 1);
  assert.equal(item.props["aria-current"], undefined);
  let choice;
  const strip = ThumbnailStrip({ label: "Examples", value: "one", options: [{ value: "one", label: "First", preview: null }, { value: "two", label: "Second", preview: null }], onChange: (value) => { choice = value; } });
  const buttons = strip.props.children;
  buttons[1].props.onClick();
  assert.equal(choice, "two");
  assert.equal(buttons[0].props["aria-pressed"], true);
  assert.equal(buttons[1].props["aria-pressed"], false);
});

test("tiles forward Shift intent for range selection without owning the range", async () => {
  const { PreviewTile } = await controlModule("PreviewTile.tsx");
  const changes = []; let presses = 0;
  const tile = PreviewTile({ title: "Example", preview: null, active: false, selected: false, onPress: () => presses++, onOpen() {}, onSelectedChange: (...args) => changes.push(args) });
  const button = find(tile, "button");
  button.props.onClick({ shiftKey: true });
  button.props.onClick({ ctrlKey: true, shiftKey: false });
  button.props.onClick({});
  find(tile, "input").props.onChange({ currentTarget: { checked: true }, nativeEvent: { shiftKey: true } });
  assert.deepEqual(changes, [[true, true], [true, false], [true, true]]);
  assert.equal(presses, 1);
  assert.equal(find(tile, "input").props.checked, false);
});

test("tiles emit optional hover intent without changing selection or assuming a content type", async () => {
  const { PreviewTile }=await controlModule("PreviewTile.tsx");const hover=[];let changes=0;
  const tile=PreviewTile({title:"Example",preview:null,active:false,selected:false,onPress(){},onOpen(){},onSelectedChange:()=>changes++,onPreviewHoverChange:value=>hover.push(value)});
  tile.props.onPointerEnter({pointerType:"touch"});tile.props.onPointerEnter({pointerType:"mouse"});tile.props.onPointerLeave();
  assert.deepEqual(hover,[true,false]);assert.equal(changes,0);
});
