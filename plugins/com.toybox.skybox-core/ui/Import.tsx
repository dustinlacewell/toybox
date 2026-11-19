/**
 * Import skies from a folder: HDR and EXR panoramas, ordinary image panoramas,
 * six-image cube maps, and cube maps unfolded into a cross or strip.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Checkbox, Pane, Select, Stack, TextInput, Workbench } from "@ldlework/toybox-sdk/ui";
import { defineImportPanel, importNamespace, validSegment, type ImportPanelCtx } from "@ldlework/toybox-sdk";

import {
  LAYOUT_LABELS, layoutFromSize, findSkies,
  type ImageLayout, type SkyCandidate, type SkyMeta,
} from "../lib/detect.js";
import { IMAGE_LAYOUTS, fileName, findImages, importSkies, measureSky, type LayoutChoice, type Measured } from "../lib/import.js";
import type { SkySource } from "../lib/sky.js";
import { SkyCanvas } from "./SkyCanvas.js";

const PREVIEW_WIDTH = 2048;
const MIME: Record<string, string> = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp" };

export default defineImportPanel(function SkyboxImport({ ctx }: { ctx: ImportPanelCtx }) {
  const [source, setSource] = useState<string>();
  const [skies, setSkies] = useState<SkyCandidate[]>([]);
  const [selected, setSelected] = useState(new Set<string>());
  const [layouts, setLayouts] = useState(new Map<string, LayoutChoice>());
  const [category, setCategory] = useState("skies");
  const [active, setActive] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string>();
  const [measured, setMeasured] = useState<{ name: string; result: Measured } | { name: string; error: string }>();
  // File bytes read for measuring are reused by the preview; object URLs are
  // revoked when the folder changes or the panel closes.
  const bytes = useRef(new Map<string, Promise<Uint8Array>>());
  const urls = useRef(new Map<string, string>());
  const forget = () => { for (const url of urls.current.values()) URL.revokeObjectURL(url); urls.current.clear(); bytes.current.clear(); };
  useEffect(() => forget, []);

  const read = (path: string): Promise<Uint8Array> => {
    if (!source) return Promise.reject(new Error("Choose a folder first"));
    let pending = bytes.current.get(path);
    if (!pending) {
      // Keep only the most recent files: a folder of 8K panoramas is gigabytes.
      if (bytes.current.size >= 6) bytes.current.delete(bytes.current.keys().next().value as string);
      pending = ctx.fs.readBytes(source, path);
      bytes.current.set(path, pending);
      pending.catch(() => bytes.current.delete(path));
    }
    return pending;
  };

  /** A sky's metadata: the layout chosen or read from the image's shape. */
  const measure = (sky: SkyCandidate, choice: LayoutChoice): Promise<Measured> => measureSky(read, sky, choice);

  const activeSky = skies.find((sky) => sky.name === active);
  const activeChoice = (active && layouts.get(active)) || "auto";
  useEffect(() => {
    if (!activeSky) { setMeasured(undefined); return; }
    let alive = true;
    setMeasured(undefined);
    measure(activeSky, activeChoice).then(
      (result) => { if (alive) setMeasured({ name: activeSky.name, result }); },
      (failure) => { if (alive) setMeasured({ name: activeSky.name, error: failure instanceof Error ? failure.message : String(failure) }); },
    );
    return () => { alive = false; };
    // `measure` closes over `source`, which changes together with `activeSky`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSky, activeChoice]);

  const preview = useMemo<SkySource | undefined>(() => {
    if (!activeSky || !measured || "error" in measured || measured.name !== activeSky.name) return undefined;
    const byName = new Map(activeSky.files.map((path) => [fileName(path), path]));
    const path = (file: string) => byName.get(file) ?? activeSky.files[0];
    return {
      meta: measured.result.meta,
      file: fileName(activeSky.files[0]),
      read: (file) => read(path(file)),
      url: async (file) => {
        const key = path(file);
        let url = urls.current.get(key);
        if (!url) {
          const data = await read(key);
          url = URL.createObjectURL(new Blob([Uint8Array.from(data)], { type: MIME[key.slice(key.lastIndexOf(".") + 1).toLowerCase()] ?? "application/octet-stream" }));
          urls.current.set(key, url);
        }
        return url;
      },
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSky, measured]);

  const choose = async () => {
    const folder = await ctx.fs.pickDirectory();
    if (!folder) return;
    setBusy(true); setError(undefined); setStatus("Looking for skies…");
    try {
      const found = findSkies(await findImages(ctx.fs, folder));
      forget();
      setSource(folder); setSkies(found); setLayouts(new Map());
      setSelected(new Set(found.filter((sky) => sky.likely).map((sky) => sky.name)));
      setActive((found.find((sky) => sky.likely) ?? found[0])?.name);
      const likely = found.filter((sky) => sky.likely).length;
      setStatus(!found.length ? "No HDR, EXR, PNG, JPG or WebP images found."
        : likely === found.length ? `${found.length} ${found.length === 1 ? "sky" : "skies"} found`
        : `${found.length} images found; ${likely} look like skies and are ticked`);
    } catch (failure) { setError(String(failure)); setStatus(""); }
    finally { setBusy(false); }
  };

  const run = async () => {
    if (!source) return;
    setBusy(true); setError(undefined);
    try {
      const root = await ctx.host.getLibraryRoot();
      if (!root) throw new Error("Choose a library first");
      const pack = importNamespace(source);
      const chosen = skies.filter((sky) => selected.has(sky.name));
      const { entries } = await importSkies({ fs: ctx.fs, libraryRoot: root }, { source, skies: chosen, layouts, category, pack, read }, setStatus);
      setStatus("Adding skies to the library…");
      await ctx.commit(entries);
    } catch (failure) { setError(String(failure)); setStatus(""); }
    finally { setBusy(false); }
  };

  const ready = !!selected.size && validSegment(category);
  useEffect(() => {
    ctx.setActions?.(<>
      <Button disabled={busy} onClick={() => void choose()}>{source ? "Change folder" : "Choose folder"}</Button>
      <Button variant="primary" disabled={busy || !ready} onClick={() => void run()}>{busy ? "Importing…" : `Import ${selected.size || "skies"}`}</Button>
    </>);
    return () => ctx.setActions?.(null);
    // The handlers read current state; re-registering on each change keeps them fresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx.setActions, source, skies, selected, layouts, category, busy]);

  const info = measured && "result" in measured && measured.name === active ? measured.result : undefined;
  const failure = measured && "error" in measured && measured.name === active ? measured.error : undefined;

  return <Workbench navigation={<Pane><Stack gap={16} style={{ padding: 16 }}>
    <h3>Import into</h3>
    <label>Category<TextInput aria-label="Category" value={category} disabled={busy} onChange={(event) => setCategory(event.currentTarget.value)} /></label>
    {!validSegment(category) && <p role="alert" style={{ color: "var(--danger)", margin: 0 }}>Enter a category name without slashes or other reserved characters.</p>}
    <p style={{ color: "var(--text-2)", margin: 0 }}>HDR and EXR panoramas, image panoramas, six-image cube maps, and cube maps unfolded into a cross or strip. Files are copied as they are.</p>
    <p style={{ color: "var(--text-2)", margin: 0 }}>Six images are read as a cube map when their names end in the faces: px/nx/py/ny/pz/nz, posx/negx…, or right/left/top/bottom/front/back.</p>
    {source && <p style={{ color: "var(--text-2)", overflowWrap: "anywhere", margin: 0 }}>{source}</p>}
  </Stack></Pane>} inspector={activeSky && <Pane><Stack gap={12} style={{ padding: 16, height: "100%", minHeight: 0 }}>
    <h3 style={{ margin: 0 }}>Preview</h3>
    <strong style={{ overflowWrap: "anywhere" }}>{activeSky.name}</strong>
    <div style={{ aspectRatio: "4 / 3", width: "100%", borderRadius: 8, overflow: "hidden", border: "1px solid var(--border)" }}>
      {preview ? <SkyCanvas sourceKey={`${source}\0${activeSky.name}\0${preview.meta.layout}`} source={preview} maxWidth={PREVIEW_WIDTH} />
        : <p role={failure ? "alert" : "status"} style={{ margin: 0, padding: 12, color: failure ? "var(--danger)" : "var(--text-2)" }}>{failure ?? "Reading image…"}</p>}
    </div>
    {activeSky.layout === "cubemap"
      ? <p style={{ color: "var(--text-2)", margin: 0 }}>{LAYOUT_LABELS.cubemap}: {activeSky.files.map(fileName).join(", ")}</p>
      : <label>Layout<Select value={activeChoice} disabled={busy}
          options={[{ value: "auto", label: info ? `Automatic — ${LAYOUT_LABELS[layoutFromSize(info.meta.width, info.meta.height).layout]}` : "Automatic" }, ...IMAGE_LAYOUTS.map((layout) => ({ value: layout, label: LAYOUT_LABELS[layout] }))]}
          onChange={(value) => setLayouts((current) => new Map(current).set(activeSky.name, value as LayoutChoice))} /></label>}
    {info && <p style={{ color: "var(--text-2)", margin: 0 }}>{info.meta.format} · {info.meta.width} × {info.meta.height}{activeSky.layout === "cubemap" ? " per face" : ""}</p>}
    {info && !info.certain && <p role="status" style={{ color: "var(--text-2)", margin: 0 }}>This image is not 2:1, so it may not be a panorama. Check the preview, or choose another layout.</p>}
    <p style={{ color: "var(--text-2)", margin: 0 }}>Drag the preview to look around.</p>
  </Stack></Pane>}>
    <Stack grow style={{ overflow: "hidden" }}>
      <div style={{ padding: "16px 20px", borderBottom: "1px solid var(--border)" }}>
        {skies.length > 0 && <Checkbox checked={selected.size === skies.length} label={`${selected.size} of ${skies.length} selected`} onChange={(checked) => { if (!busy) setSelected(checked ? new Set(skies.map((sky) => sky.name)) : new Set()); }} />}
        <p role="status" style={{ color: "var(--text-2)", marginBottom: 0 }}>{status || "Choose a folder of sky images to get started."}</p>
      </div>
      {error && <p role="alert" style={{ padding: "0 20px", color: "var(--danger)" }}>{error}</p>}
      <div style={{ overflow: "auto", flex: 1 }}>{skies.map((sky) => <div key={sky.name} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 20px", borderBottom: "1px solid var(--border)", background: active === sky.name ? "var(--bg-2)" : undefined }}>
        <Checkbox checked={selected.has(sky.name)} label="" onChange={(checked) => { if (!busy) setSelected((current) => { const next = new Set(current); if (checked) next.add(sky.name); else next.delete(sky.name); return next; }); }} />
        <Button variant="ghost" onClick={() => setActive(sky.name)} style={{ flex: 1, justifyContent: "start", minWidth: 0 }} title={sky.files.join("\n")}>
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{sky.name}</span>
        </Button>
        <span style={{ color: "var(--text-2)", fontSize: 12, whiteSpace: "nowrap" }}>{sky.layout === "cubemap" ? `Cube map · 6 × ${sky.format}` : sky.format}</span>
      </div>)}</div>
    </Stack>
  </Workbench>;
});
