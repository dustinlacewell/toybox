/**
 * Design-system barrel. The single source of truth for Toybox's primitives.
 *
 * The app imports these directly. Runtime-loaded plugins resolve them through the
 * host's import map (the `@ldlework/toybox-sdk/ui` specifier — see vite.config.ts),
 * so a plugin importing from that specifier renders with the exact same primitives,
 * against the exact same React instance, as the host — plugin UI looks native and
 * hooks work across the boundary. Plugin authors get compile-time types from the
 * package's generated, types-only `/ui` entry (emitted from this file).
 *
 * The ds CSS is imported here so the styles ship with the shared chunk; plugins
 * get the look without importing stylesheets themselves.
 */

import "./tokens.css";
import "./ds.css";

export { Button } from "./Button";
export { Checkbox } from "./Checkbox";
export { Stack } from "./Stack";
export { TextInput } from "./TextInput";
export { Select, type SelectOption } from "./Select";
export { Spinner } from "./Spinner";
export { Drawer } from "./Drawer";
export { IconButton } from "./IconButton";
export { IconStrip } from "./IconStrip";
export { Toolbar } from "./Toolbar";
