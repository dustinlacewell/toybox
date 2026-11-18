import { exportVariants } from "@ldlework/toybox-sdk";
import type { LoadedExporter } from "./pluginRegistry";
import { exportCopy, exportGlb, previewExport } from "./tauriApi";

/** Built-in file exporters use the same contract and picker as installed plugins. */
export const nativeExporters: LoadedExporter[] = (["glb", "copy"] as const).map((format) => ({
  manifest: {
    id: `toybox.${format}`, name: format === "glb" ? "Merged GLB files" : "Copy files",
    version: "1.0.0", entry: "", entryAbsPath: "", fields: [],
    // Copying original files works for every type, including plugin-defined ones.
    assetKinds: format === "glb" ? ["model", "animation"] : ["*"],
    description: format === "glb" ? "One self-contained model or animation file, with textures embedded." : "Copy the original files of the selected items to a folder.",
    permissions: {},
  },
  plugin: {
    preview: async (ctx, assets) => previewExport(exportVariants(assets, ctx.config.variantPolicy).map((a) => a.id), format, ctx.config.preserveStructure),
    run: async (ctx, assets) => (format === "glb" ? exportGlb : exportCopy)(
      exportVariants(assets, ctx.config.variantPolicy).map((a) => a.id), ctx.config.targetDir, ctx.config.preserveStructure),
  },
}));
