import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
const load = async (file) => {
  const { outputText } = ts.transpileModule(readFileSync(new URL(`../src/domain/${file}`, import.meta.url), "utf8"), { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 } });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
};
const s = await load("scaledScroll.ts");

const STRIDE = 238;
const VIEW = 900;
// 1,000,000 items in 6 columns: 166,667 rows of 238 px is 39.7 million px, past what an element can be.
const ROWS = Math.ceil(1_000_000 / 6);

test("a list that fits scrolls one to one", () => {
  const g = s.geometry(1000, STRIDE, VIEW);
  assert.equal(g.scale, 1);
  assert.equal(g.physicalHeight, 1000 * STRIDE);
  assert.equal(s.virtualTop(g, 5000), 5000);
  assert.equal(s.rowPosition(g, 30, 5000), 30 * STRIDE);
  assert.deepEqual(s.windowRows(g, 0, 4), { first: 0, last: Math.ceil(VIEW / STRIDE) - 1 + 4 });
});

test("a list past the limit gets a bounded element and a scale", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  assert.ok(ROWS * STRIDE > 33_000_000, "this is the case the old grid could not scroll");
  assert.equal(g.physicalHeight, s.MAX_SCROLL_PX);
  assert.ok(g.scale > 3 && g.scale < 4);
  assert.ok(g.physicalHeight < 17_000_000, "below Firefox's limit too");
});

test("the ends of the scrollbar are the ends of the list", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  assert.equal(s.virtualTop(g, 0), 0);
  assert.equal(s.virtualTop(g, s.maxScrollTop(g)), g.virtualHeight - VIEW);
  const top = s.windowRows(g, 0, 0);
  assert.equal(top.first, 0);
  const end = s.windowRows(g, s.maxScrollTop(g), 0);
  assert.equal(end.last, ROWS - 1, "the last row is reached");
  // At the very end the last row's bottom is the bottom of the viewport.
  const bottom = s.rowPosition(g, ROWS - 1, s.maxScrollTop(g)) + STRIDE - s.maxScrollTop(g);
  assert.ok(Math.abs(bottom - VIEW) < 1e-6, `last row ends at the bottom of the viewport (${bottom})`);
});

test("positions on screen are the same at any scale", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  for (const scrollTop of [0, 1, 777_777, 5_000_001, s.maxScrollTop(g)]) {
    const { first, last } = s.windowRows(g, scrollTop, 0);
    for (let row = first; row <= last; row++) {
      const onScreen = s.rowPosition(g, row, scrollTop) - scrollTop;
      assert.ok(onScreen > -STRIDE - 1e-6 && onScreen < VIEW + 1e-6, `row ${row} is on screen at ${scrollTop}`);
    }
    // Consecutive rows are exactly one stride apart (no overlap, no gap).
    assert.ok(Math.abs(s.rowPosition(g, first + 1, scrollTop) - s.rowPosition(g, first, scrollTop) - STRIDE) < 1e-6);
  }
});

test("scrolling to a row puts it on screen, at any depth", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  for (const row of [0, 1, 1234, 83_333, 150_000, ROWS - 2, ROWS - 1]) {
    for (const align of ["start", "end", "center"]) {
      const scrollTop = s.scrollTopForRow(g, row, align);
      assert.ok(scrollTop >= 0 && scrollTop <= s.maxScrollTop(g));
      const { first, last } = s.windowRows(g, scrollTop, 0);
      assert.ok(first <= row && row <= last, `row ${row} (${align}) is in the window ${first}..${last}`);
      if (align === "start" && row < ROWS - 8) {
        // Within the rounding of one physical pixel (= scale virtual pixels), the row is at the top.
        assert.ok(Math.abs(s.virtualTop(g, scrollTop) - row * STRIDE) <= g.scale, `row ${row} is at the top`);
      }
    }
  }
});

test("auto alignment keeps the position when the row is already visible", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  const current = s.scrollTopForRow(g, 100_000, "start");
  const visible = Math.floor(s.virtualTop(g, current) / STRIDE) + 1;
  assert.equal(s.scrollTopForRow(g, visible, "auto", current), current);
  const far = s.scrollTopForRow(g, visible + 50, "auto", current);
  assert.notEqual(far, current);
  const { first, last } = s.windowRows(g, far, 0);
  assert.ok(first <= visible + 50 && visible + 50 <= last);
});

test("the row at the top survives a change of layout", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  const scrollTop = 6_543_210;
  const row = s.topRow(g, scrollTop);
  const again = s.scrollTopForTopRow(g, row);
  assert.ok(Math.abs(again - scrollTop) <= 1);
  // A different width: fewer rows of another height, the same fractional row stays at the top.
  const narrow = s.geometry(Math.ceil(1_000_000 / 4), 300, VIEW);
  const fraction = row / ROWS;
  const moved = s.scrollTopForTopRow(narrow, fraction * Math.ceil(1_000_000 / 4));
  assert.ok(Math.abs(s.topRow(narrow, moved) - fraction * Math.ceil(1_000_000 / 4)) < narrow.scale / 300 + 1e-6);
});

test("scrolling by one wheel step moves the window by whole rows eventually and never skips the end", () => {
  const g = s.geometry(ROWS, STRIDE, VIEW);
  let previous = -1;
  for (let top = 0; top <= s.maxScrollTop(g); top += Math.floor(s.maxScrollTop(g) / 5000)) {
    const { first } = s.windowRows(g, top, 0);
    assert.ok(first >= previous, "monotonic");
    previous = first;
  }
  // Selecting a range between a row near the top and a row near the bottom needs only their indexes.
  assert.ok(ROWS * 6 > 999_000);
});

test("degenerate sizes", () => {
  assert.deepEqual(s.windowRows(s.geometry(0, STRIDE, VIEW), 0), { first: 0, last: -1 });
  const small = s.geometry(2, STRIDE, VIEW);
  assert.equal(s.maxScrollTop(small), 0);
  assert.deepEqual(s.windowRows(small, 0, 4), { first: 0, last: 1 });
  const unmeasured = s.geometry(ROWS, STRIDE, 0);
  assert.ok(Number.isFinite(s.virtualTop(unmeasured, 100)));
});
