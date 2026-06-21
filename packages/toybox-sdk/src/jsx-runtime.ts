/**
 * Local ESM wrapper exposing `react/jsx-runtime`'s named exports (`jsx`, `jsxs`,
 * `Fragment`), shared with plugins through the import map (see vite.config.ts).
 *
 * Why: plugin `.tsx` is compiled by its own tsc to
 * `import { jsx, jsxs, Fragment } from "react/jsx-runtime"` and served RAW over
 * the plugin:// origin — it never passes through Vite's import rewriting. Vite's
 * dev-optimized `react/jsx-runtime` exposes only a `default` export, so a
 * plugin's bare named imports fail in dev. This wrapper (which IS app source, so
 * Vite rewrites its own import to the real optimized dep — no import-map loop)
 * takes that default namespace and re-publishes the named bindings as real
 * static ESM exports, off the one shared React instance, in dev and prod alike.
 */

// `import * as` captures whatever shape the real module has (named exports in
// prod, a CJS default namespace in dev); `rt` normalizes to the runtime object.
import * as mod from "react/jsx-runtime";

const rt = mod as unknown as {
  default?: { jsx: unknown; jsxs: unknown; Fragment: unknown };
  jsx?: unknown;
  jsxs?: unknown;
  Fragment?: unknown;
};
const runtime = rt.default ?? (rt as { jsx: unknown; jsxs: unknown; Fragment: unknown });

export const jsx = runtime.jsx;
export const jsxs = runtime.jsxs;
export const Fragment = runtime.Fragment;
