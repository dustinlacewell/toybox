/**
 * The viewer's reference widgets and their visibility state. One source of truth
 * for which overlays exist, their toggle metadata, and defaults — the toolbar
 * renders from this and the scene reads from it.
 */

import { Axis3d, Box, Crosshair, Grid3x3, type LucideIcon } from "lucide-react";

export type WidgetKey = "grid" | "axes" | "geometryCenter" | "boundingBox";

export type WidgetVisibility = Record<WidgetKey, boolean>;

export interface WidgetMeta {
  key: WidgetKey;
  label: string;
  icon: LucideIcon;
}

/** Ordered for the toolbar. The origin axes mark the model's local origin (the
 *  point an engine pivots around); there is no separate "pivot" widget. */
export const WIDGETS: WidgetMeta[] = [
  { key: "grid", label: "Floor grid", icon: Grid3x3 },
  { key: "axes", label: "Origin axes", icon: Axis3d },
  { key: "geometryCenter", label: "Geometry center", icon: Crosshair },
  { key: "boundingBox", label: "Bounding box", icon: Box },
];

export const defaultWidgetVisibility = (): WidgetVisibility => ({
  grid: true,
  axes: true,
  geometryCenter: true,
  boundingBox: true,
});
