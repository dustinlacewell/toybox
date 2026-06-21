/**
 * The single mount point for any plugin-shipped slot UI. Lazy-loads the slot
 * module from the `plugin://` origin (so it shares the host's React via the
 * import map), wraps it in an error boundary, and renders the host `fallback`
 * if the slot has been disabled (too many throws this session) or while it
 * loads. One generic component serves both the export and import panels — there
 * are only two single-occupancy call sites, so no slot registry is needed.
 */

import { type ComponentType, type ReactNode, lazy, Suspense, useMemo } from "react";

import type { SlotComponentProps } from "@ldlework/toybox-sdk";
import { Spinner } from "@ldlework/toybox-sdk/ui";
import { loadSlotComponent } from "../../services/pluginLoader";
import { SlotErrorBoundary } from "./SlotErrorBoundary";
import { isSlotDisabled } from "./slotDisableRegistry";

interface Props<Ctx> {
  pluginId: string;
  moduleRel: string;
  ctx: Ctx;
  fallback: ReactNode;
}

/** Cache of lazy components keyed by `${id}::${rel}` so a slot isn't re-imported
 *  on every render (React.lazy must be created once, not inline). */
const lazyByKey = new Map<string, ComponentType<SlotComponentProps<unknown>>>();

function lazySlot(pluginId: string, moduleRel: string) {
  const k = `${pluginId}::${moduleRel}`;
  let Comp = lazyByKey.get(k);
  if (!Comp) {
    Comp = lazy(async () => ({ default: await loadSlotComponent(pluginId, moduleRel) }));
    lazyByKey.set(k, Comp);
  }
  return Comp;
}

export function PluginSlot<Ctx>({ pluginId, moduleRel, ctx, fallback }: Props<Ctx>) {
  const disabled = isSlotDisabled(pluginId, moduleRel);
  const Comp = useMemo(
    () => (disabled ? null : lazySlot(pluginId, moduleRel)),
    [disabled, pluginId, moduleRel],
  );

  if (!Comp) return <>{fallback}</>;

  return (
    <SlotErrorBoundary pluginId={pluginId} moduleRel={moduleRel} fallback={fallback}>
      <Suspense fallback={<Spinner />}>
        <Comp ctx={ctx as unknown} />
      </Suspense>
    </SlotErrorBoundary>
  );
}
