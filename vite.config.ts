import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { viteImportMaps } from "vite-import-maps";

// The SDK package (@ldlework/toybox-sdk) is the plugin *contract* — the app and
// runtime plugins resolve it to one shared chunk via the import map. The design
// system lives in the app (src/ds); the `@ldlework/toybox-sdk/ui` specifier is a
// runtime alias onto it, so plugin UI shares the host's exact primitives + React.
const sdkRoot = fileURLToPath(new URL("./packages/toybox-sdk/src", import.meta.url));
const toyboxSdk = `${sdkRoot}/index.ts`;
const toyboxUi = fileURLToPath(new URL("./src/ds/index.ts", import.meta.url));
// React is CommonJS, so Vite's dev-optimized deps expose only a `default` export
// — fine for the app (Vite rewrites its source imports) but not for raw plugin
// modules served over plugin://, which need real named ESM exports. These SDK
// wrappers re-publish the named bindings off the one shared React instance; the
// import map points the bare specifiers at them (only for browser/plugin
// resolution — the app and the wrappers themselves import the real `react`).
const reactShim = `${sdkRoot}/react.ts`;
const jsxRuntimeShim = `${sdkRoot}/jsx-runtime.ts`;

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  // Resolve the bare `toybox` specifier (used by both the app and runtime
  // plugins) to the SDK source, so the import-maps plugin tracks it and emits
  // a shared chunk + map entry. One instance across the app/plugin boundary.
  resolve: {
    alias: {
      "@ldlework/toybox-sdk/ui": toyboxUi,
      "@ldlework/toybox-sdk": toyboxSdk,
    },
  },
  plugins: [
    react(),
    // Shared-dependency import map. Both the app and dynamically-loaded plugins
    // (served from the plugin:// origin) resolve these bare specifiers to ONE
    // bundled chunk apiece — a single instance across the app/plugin boundary.
    // three: r3f's instanceof checks keep working. react/react-dom/jsx-runtime:
    // plugin-shipped React components run on the host's exact React instance, so
    // hooks work and there's no "multiple copies of React". toybox + toybox/ui:
    // the plugin contract and the app's design-system primitives (src/ds), so
    // plugin UI looks native.
    // Injected as <script type="importmap"> in dev and prod.
    viteImportMaps({
      imports: [
        "three",
        "react-dom",
        "react-dom/client",
        "lucide-react",
        // react + jsx-runtime point at named-export wrappers (see above) so raw
        // plugin modules resolve `{ useState }` / `{ jsx }` in dev and prod.
        { name: "react", entry: reactShim },
        { name: "react/jsx-runtime", entry: jsxRuntimeShim },
        { name: "@ldlework/toybox-sdk", entry: toyboxSdk },
        { name: "@ldlework/toybox-sdk/ui", entry: toyboxUi },
      ],
    }),
  ],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
