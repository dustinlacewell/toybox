import { exportVariants } from "@ldlework/toybox-sdk";
import type { LoadedExporter } from "./pluginRegistry";
import { exportCopy, exportGlb, previewExport } from "./tauriApi";

/** Built-in file exporters use the same contract and picker as installed plugins. */
export const nativeExporters: LoadedExporter[] = (["glb", "copy"] as const).map((format) => ({
  manifest: {
    id: `toybox.${format}`, name: format === "glb" ? "Merged GLB files" : "Copy files",
    version: "1.0.0", kind: "exporter", entry: "", entryAbsPath: "", fields: [],
    assetKinds: format === "glb" ? ["model", "animation"] : ["model", "material", "animation", "audio"],
    description: format === "glb" ? "One self-contained model or animation file, with textures embedded." : "Copy models, animation clips, audio or material maps to a folder.",
    permissions: {},
  },
  plugin: {
    preview: async (ctx, assets) => previewExport(exportVariants(assets, ctx.config.variantPolicy).map((a) => a.id), format, ctx.config.preserveStructure),
    run: async (ctx, assets) => (format === "glb" ? exportGlb : exportCopy)(
      exportVariants(assets, ctx.config.variantPolicy).map((a) => a.id), ctx.config.targetDir, ctx.config.preserveStructure),
  },
}));
