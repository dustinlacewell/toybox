import { defineConfig } from "astro/config";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "url";
import { readFileSync } from "fs";
import path from "path";

const root = path.dirname(fileURLToPath(import.meta.url));
const pkg = JSON.parse(readFileSync(path.resolve(root, "../package.json"), "utf-8"));

const appSrc = path.resolve(root, "../src");
const shimDir = path.resolve(root, "src/demo/shims");
const norm = (p) => p.replace(/\\/g, "/");

/**
 * Swap only the three app service modules that touch Tauri for browser-native
 * shims, wherever they're imported (the real components reach them through
 * relative `../services/...` paths). A resolveId hook is used rather than a
 * Vite `alias` because aliases match the import *specifier*, not the resolved
 * file — we want to redirect by final on-disk path so any importer is caught.
 */
// Map of app module (relative to src) -> demo shim file (relative to shimDir).
// The three services are swapped because they call Tauri; useThumbUrl is
// swapped because the app appends a `?v=<state>` cache-buster to the thumbnail
// URL — harmless on a Tauri asset:// URL, but it makes an in-memory blob: URL
// unresolvable, so the demo reads the blob URL straight from the store.
const SWAPS = {
  "services/assetUrl.ts": "assetUrl.ts",
  "services/tauriApi.ts": "tauriApi.ts",
  "services/reveal.ts": "reveal.ts",
  "components/useThumbUrl.ts": "useThumbUrl.ts",
};
function swapTauriSeams() {
  return {
    name: "toybox-demo-swap-tauri-seams",
    enforce: "pre",
    async resolveId(source, importer, options) {
      const resolved = await this.resolve(source, importer, { ...options, skipSelf: true });
      if (!resolved) return null;
      const file = norm(resolved.id).split("?")[0];
      for (const [appRel, shimFile] of Object.entries(SWAPS)) {
        if (file === norm(path.resolve(appSrc, appRel))) {
          return norm(path.resolve(shimDir, shimFile));
        }
      }
      return null;
    },
  };
}

export default defineConfig({
  site: "https://toybox.ldlework.com",
  output: "static",
  integrations: [react()],
  vite: {
    plugins: [swapTauriSeams(), tailwindcss()],
    define: {
      __APP_VERSION__: JSON.stringify(pkg.version),
    },
    resolve: {
      // App components import the plugin SDK by bare specifier. Resolve both
      // entries from source — matching the app's own vite.config — so the demo
      // build never hits the package's dist/ (the contract → package source;
      // `/ui` → the app's design system, src/ds). The `/ui` entry must precede
      // the bare name so the longer specifier matches first.
      alias: [
        { find: "@ldlework/toybox-sdk/ui", replacement: path.resolve(appSrc, "ds/index.ts") },
        { find: "@ldlework/toybox-sdk", replacement: path.resolve(root, "../packages/toybox-sdk/src/index.ts") },
        { find: "@app", replacement: appSrc },
      ],
      // App components live at repo-root ../src and are pulled in through the
      // @app alias. Their bare dependency imports would otherwise resolve from
      // the repo root, whose node_modules doesn't exist in CI (only site/ is
      // installed there). dedupe forces these shared packages — all declared in
      // site/package.json — to resolve from site/node_modules regardless of the
      // importing file's location.
      dedupe: [
        "react",
        "react-dom",
        "three",
        "zustand",
        "@react-three/fiber",
        "@react-three/drei",
        "@tanstack/react-virtual",
        "lucide-react",
      ],
    },
  },
});
