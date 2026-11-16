import { useEffect, useMemo, useState, type ReactNode } from "react";
import { variantInfo, MATERIAL_ROLE_LABELS, type PanelWidgets } from "@ldlework/toybox-sdk";
import { Button, Checkbox, ImageViewport, NavigationItem, Pane, PropertyRow, Segmented, Select, TextInput, ThumbnailStrip, Workbench, type ImageChannel, type ImageScale } from "@ldlework/toybox-sdk/ui";
import { materialGroups } from "./variants.js";
import { DraftPreview } from "./DraftPreview.js";
import { canImport, problemsFor, type ReviewItem } from "./edit.js";
import { draftMaps } from "./entry.js";
import type { ReviewHandlers } from "./handlers.js";
import { MaterialList } from "./MaterialList.js";
import { Thumb } from "./Thumb.js";
import { ROLE_OPTIONS, roleFromKey, USE_OPTIONS, useFromKey, useKey } from "./uses.js";
import type { ThumbLoader } from "./thumbs.js";

interface Props extends ReviewHandlers {
  items: ReviewItem[]; pack: string; category: string; loader: ThumbLoader;
  Preview: PanelWidgets["MaterialPreview"]; initialSelected: number;
  onClose: () => void; onImport: () => void; onScan?: () => void;
  source?: string; setActions?: (actions: ReactNode) => void;
}
export function ReviewDialog({ items, pack, category, loader, Preview, initialSelected, onImport, onScan, source, setActions, onInclude, onIncludeAll, onRename, onUse, onChannel }: Props) {
  const [selected, setSelected] = useState(initialSelected);
  const [fileIndex, setFileIndex] = useState(-1);
  const [channel, setChannel] = useState<ImageChannel>("all");
  const [scale, setScale] = useState<ImageScale>("fit");
  const [policy, setPolicy] = useState("all");
  const problems = useMemo(() => items.map((_, i) => problemsFor(items, i, pack, category)), [items, pack, category]);
  const groups = useMemo(() => materialGroups(items), [items]);
  const variants = groups.find((g) => g.includes(selected)) ?? [];
  const attention = groups.filter((g) => g.some((i) => items[i].included && problems[i].length));
  const readyGroups = groups.filter((g) => !attention.includes(g));
  const count = groups.filter((g) => g.some((i) => items[i].included && !problems[i].length)).length;
  const ready = canImport(items, pack, category);
  const open = items[selected];
  const file = open?.draft.files[fileIndex];
  const pick = (index: number) => { setSelected(index); setFileIndex(-1); setChannel("all"); };
  const actions = <><Select value={policy} options={[{ value: "all", label: "All sizes" }, { value: "best", label: "Best only" }]} onChange={(value) => {
    setPolicy(value); onIncludeAll(items.map((_, i) => i), value === "all"); if (value === "best") onIncludeAll(groups.map((g) => g[0]), true);
  }} /><Button variant="primary" disabled={!ready} onClick={onImport}>Add {count} to library</Button></>;
  useEffect(() => {
    setActions?.(actions);
    return () => setActions?.(null);
    // Actions capture the current edits; host rerenders keep these data references.
  }, [setActions, policy, items, pack, category]);
  const maps = open ? draftMaps(open.draft).maps : [];
  return <Workbench navigation={<>
    <h2 className="import-review__title">{count} ready. {attention.length} need a look.</h2>
    <p className="import-review__summary">{source?.split(/[\\/]/).pop()}</p>
    {attention.length > 0 && <><div className="import-review__section">Needs a look</div><MaterialList items={items} problems={problems} indexes={attention.flat()} selected={selected} loader={loader} onSelect={pick} onInclude={onInclude} /></>}
    <div className="import-review__section">Ready</div><MaterialList items={items} problems={problems} indexes={readyGroups.flat()} selected={selected} loader={loader} onSelect={pick} onInclude={onInclude} />
    <div style={{ marginTop: "auto", padding: 8 }}><PropertyRow label="Pack" value={pack} /><PropertyRow label="Category" value={category} />{onScan && <Button variant="ghost" onClick={onScan}>Choose another folder</Button>}</div>
  </>} inspector={<Pane footer={open && <><Button onClick={() => onIncludeAll(variants, false)}>Leave out</Button><Button variant="primary" onClick={() => {
    const index = groups.findIndex((g) => g.includes(selected)); const next = groups[(index + 1) % groups.length]; if (next) pick(next[0]);
  }}>Next material</Button></>}>
    {open && (file ? <>
      <h2 className="import-review__title">{file.fileName}</h2><p className="import-review__summary">{variantInfo(open.draft.name, "material").label}</p>
      <div className="import-review__section">This file is</div>
      {USE_OPTIONS.map((option) => <NavigationItem key={option.value} label={option.label} active={useKey(file.use) === option.value} detail={useKey(file.use) === option.value ? "✓" : undefined} onPress={() => onUse(selected, fileIndex, useFromKey(option.value))} />)}
      {file.use.use === "channels" && <><div className="import-review__section">Each channel</div>{(["r", "g", "b", "a"] as const).map((c) => <div className="import-review__channel" key={c}><span>{({ r: "Red", g: "Green", b: "Blue", a: "Alpha" })[c]}</span><Select value={file.use.use === "channels" ? file.use.channels[c] ?? "" : ""} options={ROLE_OPTIONS} onChange={(value) => onChannel(selected, fileIndex, c, roleFromKey(value))} /></div>)}</>}
    </> : <>
      <TextInput aria-label="Material name" value={open.draft.name} onChange={(e) => onRename(selected, e.currentTarget.value)} />
      {problems[selected].map((problem) => <p key={problem} className="import-review__problem">{problem}</p>)}
      <div className="import-review__section">Result</div>
      {maps.map((map, i) => <PropertyRow key={i} label={MATERIAL_ROLE_LABELS[map.role]} value={`${map.file}${map.channel ? `, ${map.channel.toUpperCase()}` : ""}`} onPress={() => setFileIndex(open.draft.files.findIndex((f) => f.fileName === map.file))} />)}
      <div className="import-review__section">Sizes to bring in</div><div className="import-review__checks">{variants.map((i) => <Checkbox key={i} label={variantInfo(items[i].draft.name, "material").label} checked={items[i].included} onChange={(included) => onInclude(i, included)} />)}</div>
    </>)}
  </Pane>} header={!setActions && <div style={{ display: "flex", padding: 12, gap: 8 }}>{actions}</div>} footer={open && <ThumbnailStrip label="Source files" value={String(fileIndex)} onChange={(value) => { setFileIndex(Number(value)); setChannel("all"); }} options={[
    { value: "-1", label: "Result", preview: <span style={{ fontSize: 36, color: "var(--text-1)" }}>◉</span> },
    ...open.draft.files.map((f, i) => ({ value: String(i), label: f.fileName, detail: USE_OPTIONS.find((o) => o.value === useKey(f.use))?.label, preview: <Thumb loader={loader} relPath={f.relPath} box={108} size={128} /> })),
  ]} />}>
    <div className="import-review__stage">
      {open && (file ? <SourceImage loader={loader} path={file.relPath} channel={channel} scale={scale} /> : <DraftPreview draft={open.draft} loader={loader} Preview={Preview} controls={variants.length > 1 && <Segmented value={String(selected)} options={variants.map((i) => ({ value: String(i), label: variantInfo(items[i].draft.name, "material").label }))} onChange={(v) => pick(Number(v))} />} />)}
      {file && <div className="import-review__controls">
        {file && <><Segmented value={channel} options={[{ value: "all", label: "All" }, { value: "r", label: "R" }, { value: "g", label: "G" }, { value: "b", label: "B" }, { value: "a", label: "A" }]} onChange={(v) => setChannel(v as ImageChannel)} /><Segmented value={scale} options={[{ value: "fit", label: "Fit" }, { value: "actual", label: "1:1" }]} onChange={setScale} /></>}
        {variants.length > 1 && <Segmented value={String(selected)} options={variants.map((i) => ({ value: String(i), label: variantInfo(items[i].draft.name, "material").label }))} onChange={(v) => pick(Number(v))} />}
      </div>}
    </div>
  </Workbench>;
}

function SourceImage({ loader, path, channel, scale }: { loader: ThumbLoader; path: string; channel: ImageChannel; scale: ImageScale }) {
  const [url, setUrl] = useState<string>();
  const [error, setError] = useState(false);
  useEffect(() => { let alive = true; setUrl(undefined); setError(false); void loader.load(path, scale === "actual" ? 0xffffffff : 2048).then((value) => { if (alive) setUrl(value); }, () => { if (alive) setError(true); }); return () => { alive = false; }; }, [loader, path, scale]);
  return url ? <ImageViewport src={url} alt={path} channel={channel} scale={scale} /> : <p className="import-review__summary">{error ? "Image unavailable" : "Loading image…"}</p>;
}
