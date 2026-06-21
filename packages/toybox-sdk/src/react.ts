/**
 * Local ESM wrapper exposing React's named exports, shared with plugins through
 * the import map (see vite.config.ts).
 *
 * Why: React 19 is CommonJS, so Vite's dev-optimized `react` exposes only a
 * `default` export (named bindings are pulled off it by Vite's transform of app
 * SOURCE). Plugin `.tsx` is served raw over the plugin:// origin and never
 * transformed, so its bare `import { useState } from "react"` finds no named
 * export in dev. This wrapper — which IS app source, so Vite resolves its own
 * `import React from "react"` to the real optimized dep (no import-map loop) —
 * re-publishes React's named API as static ESM, off the one shared instance.
 *
 * The set mirrors React 19's public named exports; it is stable across the 19.x
 * line. If a future React adds an export a plugin needs, add it here.
 */

import React from "react";

export default React;

export const {
  Children,
  Component,
  Fragment,
  Profiler,
  PureComponent,
  StrictMode,
  Suspense,
  cloneElement,
  createContext,
  createElement,
  createRef,
  forwardRef,
  isValidElement,
  lazy,
  memo,
  startTransition,
  use,
  useActionState,
  useCallback,
  useContext,
  useDebugValue,
  useDeferredValue,
  useEffect,
  useId,
  useImperativeHandle,
  useInsertionEffect,
  useLayoutEffect,
  useMemo,
  useOptimistic,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
  useTransition,
  version,
} = React;
