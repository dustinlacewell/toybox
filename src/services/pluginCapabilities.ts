/**
 * What a plugin does, as far as its manifest shows. There are no plugin kinds:
 * what a plugin ships decides what it is. What only its entry module can say
 * (exporting, rig profiles, extension contributions) is known after loading.
 * Pure: no host imports.
 */

interface CapabilitySource {
  entry?: string;
  assetTypes?: unknown[];
  ui?: { importPanel?: string };
}

/** The plugin declares library types. */
export const declaresTypes = (manifest: CapabilitySource): boolean => !!manifest.assetTypes?.length;

/** The plugin ships an import panel, so it is offered on the import screen. */
export const isImporter = (manifest: CapabilitySource): boolean => !!manifest.ui?.importPanel;

/** The plugin has an entry module to load. */
export const needsEntry = (manifest: CapabilitySource): boolean => !!manifest.entry;

/** The plugin declares a type its own entry module implements, so the module
 *  must provide `types`. A native declaration needs nothing from the module. */
export const implementsTypes = (manifest: { assetTypes?: { implementation?: string }[] }): boolean =>
  !!manifest.assetTypes?.some((type) => type?.implementation !== "native");

/** Whether `plugin`'s `ui.assetPanel` is shown as a viewer tab for `asset`: the
 *  item came from that plugin (or one it replaces), and it is not an item of a
 *  type that plugin declares itself, whose Viewer is already the plugin's own UI. */
export const showsAssetPanel = (
  plugin: { id: string; replaces?: string[]; ui?: { assetPanel?: string }; assetTypes?: { id: string }[] },
  asset: { kind: string; type?: string; origin?: { plugin?: string } },
): boolean => {
  const origin = asset.origin?.plugin;
  if (!plugin.ui?.assetPanel || !origin) return false;
  if (plugin.id !== origin && !plugin.replaces?.includes(origin)) return false;
  return !(asset.kind === "plugin" && plugin.assetTypes?.some((type) => type?.id === asset.type));
};
