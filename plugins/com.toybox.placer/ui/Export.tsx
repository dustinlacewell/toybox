/**
 * The placer's export-drawer panel — the reference plugin-shipped UI. Renders its
 * own config (per-asset format, project subfolder, target asset_library.json)
 * using the host's design-system primitives via `toybox/ui`, so it looks native,
 * and pushes the collected config up through `ctx.setConfig`. The placer's `run`
 * (in ../index.ts) consumes `{ format, subDir, libraryJson }` unchanged.
 */

import { useEffect, useState } from "react";

import { Stack, Button, TextInput } from "@ldlework/toybox-sdk/ui";
import { defineExportPanel, type ExportPanelCtx } from "@ldlework/toybox-sdk";

type Format = "glb" | "copy";

export default defineExportPanel(function ExportPanel({ ctx }: { ctx: ExportPanelCtx }) {
  const [format, setFormat] = useState<Format>("glb");
  const [subDir, setSubDir] = useState("assets/exported");
  const [libraryJson, setLibraryJson] = useState<string | null>(null);

  // Push config up whenever it changes; ready once a library file is chosen.
  useEffect(() => {
    ctx.setConfig({ format, subDir, libraryJson }, !!libraryJson);
  }, [ctx, format, subDir, libraryJson]);

  const pickLibrary = async () => {
    const chosen = await ctx.host.pickSaveFile("asset_library.json");
    if (chosen) setLibraryJson(chosen);
  };

  return (
    <Stack gap={12}>
      <Field label="Per-asset file format">
        <FormatCard
          active={format === "glb"}
          title="Merged .glb"
          desc="One embedded binary per asset. Cleanest to drop into a project."
          onClick={() => setFormat("glb")}
        />
        <FormatCard
          active={format === "copy"}
          title="Loose .gltf copy"
          desc="Editable .gltf + .bin + textures in-project."
          onClick={() => setFormat("copy")}
        />
      </Field>

      <Field label="Project subfolder" hint="Relative to the project; also the res:// prefix for each asset's id.">
        <TextInput value={subDir} onChange={(e) => setSubDir(e.currentTarget.value)} placeholder="assets/exported" />
      </Field>

      <Field label="asset_library.json" hint="The addon's library file to create or merge.">
        <Stack dir="row" gap={8} align="center">
          <Button onClick={pickLibrary}>Choose file…</Button>
          <span className="export__path" title={libraryJson ?? ""}>
            {libraryJson ?? "No file chosen — create or merge"}
          </span>
        </Stack>
      </Field>
    </Stack>
  );
});

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="export__section">
      <div className="export__label">{label}</div>
      {children}
      {hint && <div className="export__hint">{hint}</div>}
    </section>
  );
}

function FormatCard({
  active,
  title,
  desc,
  onClick,
}: {
  active: boolean;
  title: string;
  desc: string;
  onClick: () => void;
}) {
  return (
    <button className={`export__mode ${active ? "is-active" : ""}`} onClick={onClick}>
      <div className="export__mode-title">{title}</div>
      <div className="export__mode-desc">{desc}</div>
    </button>
  );
}
