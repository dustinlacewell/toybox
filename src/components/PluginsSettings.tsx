/**
 * The Plugins page of Settings, laid out as an add-on list: every installed
 * plugin is a compact row with a checkbox to turn it on or off, and opens to
 * its details and the preferences of the asset types it provides.
 */

import { useEffect, useState, type ReactNode } from "react";
import { Button, Checkbox, Segmented, Select, Stack, TextInput } from "@ldlework/toybox-sdk/ui";
import type { SettingSpec } from "@ldlework/toybox-sdk";
import { ChevronRight, Search, TriangleAlert } from "lucide-react";

import { isActive } from "../services/pluginActive";
import { naturalList, providesText, type Provided } from "../services/pluginProvides";
import { pluginUrl } from "../services/pluginLoader";
import type { PluginLoadError } from "../services/pluginRegistry";
import { pickFile } from "../services/pickFile";
import { pickDirectory } from "../services/pickDirectory";
import { getFbx2gltfPath, getPluginSettings, setFbx2gltfPath, setPluginEnabled, setPluginSetting, type PluginManifestDto } from "../services/tauriApi";
import { assetTypeEntry } from "../state/assetTypes";
import { usePreviewPreferences } from "../state/previewPreferences";

const PERMISSION_LABELS: [keyof PluginManifestDto["permissions"], string][] = [
  ["fsRead", "Read files"], ["fsWrite", "Write files"], ["rustConvert", "Convert files"], ["rustExport", "Export files"],
  ["runTools", "Run programs"], ["localNetwork", "Local connections"],
];

/** Preferences the host keeps for a plugin, shown under the plugin they belong
 *  to: the FBX converter under every plugin allowed to convert files, the
 *  preview shape under the plugin that declares materials. */
const HOST_SETTINGS: { key: string; applies: (manifest: PluginManifestDto) => boolean; render: () => ReactNode }[] = [
  { key: "fbx-converter", applies: (manifest) => !!manifest.permissions?.rustConvert, render: () => <FbxConverterSetting /> },
  { key: "material-shape", applies: (manifest) => (manifest.assetTypes ?? []).some((type) => type?.id === "material"), render: () => <MaterialShapeSetting /> },
];

interface Props {
  plugins: PluginManifestDto[];
  errors: PluginLoadError[];
  /** What each loaded plugin provides, by plugin id. */
  provided: Record<string, Provided>;
  /** Discover plugins again so a change takes effect at once. */
  onChanged: () => Promise<void>;
}

/** An add-on list: a search box and an "enabled only" filter over one compact
 *  row per plugin — disclosure arrow, checkbox, name. */
export function PluginsSettings({ plugins, errors, provided, onChanged }: Props) {
  const [busy, setBusy] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const [query, setQuery] = useState("");
  const [enabledOnly, setEnabledOnly] = useState(false);
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(undefined), 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);
  // Plugins are shown by their internal name: the id without the bundled prefix.
  const nameOf = (id: string) => id.replace(/^com\.toybox\./, "");
  const providesOf = (manifest: PluginManifestDto) => providesText(manifest, (id) => assetTypeEntry(id).label,
    Object.keys(manifest.dependencies ?? {}).map(nameOf), isActive(manifest) ? provided[manifest.id] : undefined);

  const toggle = async (manifest: PluginManifestDto, enabled: boolean) => {
    setBusy(manifest.id); setFailure(undefined); setNotice(undefined);
    try {
      const changed = await setPluginEnabled(manifest.id, enabled);
      const others = enabled ? changed.filter((id) => id !== manifest.id) : [];
      if (others.length) setNotice(`Also turned on ${naturalList(others.map(nameOf))}.`);
      await onChanged();
    }
    // A refused turn-off comes with its own sentence.
    catch (error) { setFailure(enabled ? `${nameOf(manifest.id)} could not be turned on. ${String(error)}` : String(error)); }
    finally { setBusy(undefined); }
  };
  const expand = (id: string) => setOpen((current) => { const next = new Set(current); if (!next.delete(id)) next.add(id); return next; });

  const needle = query.trim().toLowerCase();
  const shown = [...plugins].sort((a, b) => nameOf(a.id).localeCompare(nameOf(b.id))).filter((manifest) => (!enabledOnly || manifest.enabled !== false)
    && (!needle || [manifest.name, manifest.id, manifest.description ?? "", providesOf(manifest)].some((text) => text.toLowerCase().includes(needle))));

  return <>
    <h2>Plugins</h2>
    <p className="settings__description">Plugins provide Toybox's asset types, importers and exporters. Turning one off hides its sections and items from the library. Nothing is deleted, and they return when it is turned back on.</p>
    <div className="addons__bar">
      <div className="addons__search">
        <Search size={14} aria-hidden="true" />
        <TextInput type="search" value={query} placeholder="Search plugins" aria-label="Search plugins" onChange={(event) => setQuery(event.currentTarget.value)} />
      </div>
      <Checkbox checked={enabledOnly} onChange={setEnabledOnly} label="Enabled only" />
    </div>
    {notice && <p className="addons__notice" role="status">{notice}</p>}
    {failure && <p className="settings__error" role="alert">{failure}</p>}
    {!shown.length && <p className="addons__empty" role="status">{plugins.length ? "No plugins match." : "No plugins are installed."}</p>}
    <ul className="addons" aria-label="Installed plugins">
      {shown.map((manifest) => {
        const enabled = manifest.enabled !== false;
        const active = isActive(manifest);
        const expanded = open.has(manifest.id);
        const needers = (manifest.requiredBy ?? []).map(nameOf);
        const neededBy = needers.length ? `Needed by ${naturalList(needers)}` : undefined;
        const needs = Object.entries(manifest.dependencies ?? {}).map(([id, range]) => {
          const accepted = range && range !== "*" ? range : undefined;
          if (!plugins.some((plugin) => plugin.id === id)) return `${id} (${[accepted, "not installed"].filter(Boolean).join(", ")})`;
          return `${nameOf(id)}${accepted ? ` (${accepted})` : ""}`;
        });
        const settings = HOST_SETTINGS.filter(({ applies }) => applies(manifest));
        const own = (manifest.settings ?? []).filter((spec) => spec && typeof spec.key === "string" && typeof spec.label === "string");
        const problems = errors.filter((error) => error.manifest.id === manifest.id);
        const permissions = PERMISSION_LABELS.filter(([key]) => manifest.permissions?.[key]).map(([, label]) => label);
        const panel = `addon-${manifest.id}`;
        return <li key={manifest.id} className={`addon${active ? "" : " is-off"}${expanded ? " is-open" : ""}`}>
          <div className="addon__row">
            <button type="button" className="addon__disclosure" aria-expanded={expanded} aria-controls={panel}
              aria-label={`${expanded ? "Hide" : "Show"} details of ${nameOf(manifest.id)}`} onClick={() => expand(manifest.id)}>
              <ChevronRight size={14} aria-hidden="true" />
            </button>
            <input type="checkbox" className="addon__check" checked={enabled} disabled={busy !== undefined || (enabled && !!neededBy)} aria-label={`${nameOf(manifest.id)} enabled`} title={enabled ? neededBy : undefined}
              onChange={(event) => void toggle(manifest, event.currentTarget.checked)} />
            <button type="button" className="addon__name" tabIndex={-1} aria-hidden="true" onClick={() => expand(manifest.id)}>
              {manifest.icon ? <img className="addon__icon" src={pluginUrl(manifest.id, manifest.icon)} alt="" /> : <span className="addon__icon" />}
              <span className="addon__title addon__mono">{nameOf(manifest.id)}</span>
              {neededBy && <span className="addon__note">{neededBy}</span>}
              <span className="addon__provides">{busy === manifest.id ? "Applying…" : providesOf(manifest)}</span>
            </button>
            {problems.length > 0 && <TriangleAlert className="addon__warning" size={14} aria-label="This plugin reported a problem" />}
          </div>
          {expanded && <div className="addon__panel" id={panel}>
            <dl className="addon__details">
              {manifest.description && <><dt>Description</dt><dd>{manifest.description}</dd></>}
              <dt>Provides</dt><dd>{providesOf(manifest)}</dd>
              {needs.length > 0 && <><dt>Requires</dt><dd>{needs.join(", ")}</dd></>}
              {needers.length > 0 && <><dt>Required by</dt><dd>{needers.join(", ")}</dd></>}
              <dt>Version</dt><dd>{manifest.version}</dd>
              <dt>Name</dt><dd>{manifest.name}</dd>
              <dt>Identifier</dt><dd className="addon__mono">{manifest.id}</dd>
              <dt>Permissions</dt><dd>{permissions.length ? permissions.join(", ") : "None"}</dd>
            </dl>
            {problems.map((problem) => <p key={problem.error} className="settings__error" role="alert">{problem.error}</p>)}
            {(settings.length > 0 || own.length > 0) && <div className="addon__preferences">
              <h3>Preferences</h3>
              {active ? <>
                {own.length > 0 && <PluginOwnSettings pluginId={manifest.id} specs={own} />}
                {settings.map(({ key, render }) => <div key={key} className="addon__setting">{render()}</div>)}
              </> : <p className="settings__description">Turn this plugin on to change its preferences.</p>}
            </div>}
          </div>}
        </li>;
      })}
    </ul>
  </>;
}

/** The settings a plugin declares in its manifest: paths to external programs,
 *  defaults and switches. Each change is stored at once. */
function PluginOwnSettings({ pluginId, specs }: { pluginId: string; specs: SettingSpec[] }) {
  const [values, setValues] = useState<Record<string, unknown>>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    let alive = true;
    getPluginSettings(pluginId).then((loaded) => { if (alive) setValues(loaded); }, (failure) => { if (alive) setError(String(failure)); });
    return () => { alive = false; };
  }, [pluginId]);
  // Store, then read back: a cleared value shows its default again.
  const store = async (key: string, value: unknown) => {
    setError(undefined);
    try { await setPluginSetting(pluginId, key, value); setValues(await getPluginSettings(pluginId)); }
    catch (failure) { setError(String(failure)); }
  };
  const choose = async (spec: SettingSpec) => {
    try { const chosen = spec.type === "directory" ? await pickDirectory() : await pickFile(); if (chosen) await store(spec.key, chosen); }
    catch (failure) { setError(String(failure)); }
  };
  if (!values) return <p className="settings__description" role="status">{error ?? "Loading…"}</p>;
  return <>
    {specs.map((spec) => {
      const value = values[spec.key];
      return <div key={spec.key} className="addon__setting">
        {spec.type !== "checkbox" && <h4>{spec.label}</h4>}
        {spec.hint && spec.type !== "checkbox" && <p className="settings__description">{spec.hint}</p>}
        {(spec.type === "file" || spec.type === "directory") && <>
          <p className="settings__path">{typeof value === "string" && value ? value : "Not set"}</p>
          <Stack dir="row" gap={8}>
            <Button onClick={() => void choose(spec)}>{spec.type === "file" ? "Choose file…" : "Choose folder…"}</Button>
            <Button variant="ghost" onClick={() => void store(spec.key, null)}>{spec.default ? "Use default" : "Clear"}</Button>
          </Stack>
        </>}
        {spec.type === "text" && <TextInput aria-label={spec.label} defaultValue={typeof value === "string" ? value : ""} placeholder={spec.placeholder}
          key={String(value ?? "")} onBlur={(event) => { if (event.currentTarget.value !== (value ?? "")) void store(spec.key, event.currentTarget.value); }} />}
        {spec.type === "select" && <Select value={typeof value === "string" ? value : ""} options={spec.options.map(({ value: option, label }) => ({ value: option, label }))}
          onChange={(next) => void store(spec.key, next)} />}
        {spec.type === "checkbox" && <><Checkbox checked={value === true} label={spec.label} onChange={(checked) => void store(spec.key, checked)} />
          {spec.hint && <p className="settings__description">{spec.hint}</p>}</>}
      </div>;
    })}
    {error && <p className="settings__error" role="alert">{error}</p>}
  </>;
}

/** The FBX2glTF executable the model importer converts `.fbx` files with. */
function FbxConverterSetting() {
  const [path, setPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  useEffect(() => {
    let alive = true;
    getFbx2gltfPath().then((value) => { if (alive) setPath(value); }, (failure) => { if (alive) setError(String(failure)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, []);
  const change = async (clear = false) => {
    setError(undefined);
    try {
      const chosen = clear ? null : await pickFile();
      if (!clear && !chosen) return;
      await setFbx2gltfPath(chosen);
      setPath(chosen);
    } catch (failure) { setError(String(failure)); }
  };
  return <>
    <h4>FBX converter</h4>
    <p className="settings__description">Choose an FBX2glTF executable to enable FBX imports.</p>
    <p className="settings__path">{loading ? "Loading…" : path ?? "No converter selected"}</p>
    <Stack dir="row" gap={8}><Button disabled={loading} onClick={() => void change()}>Choose FBX2glTF…</Button>
      {path && <Button variant="ghost" disabled={loading} onClick={() => void change(true)}>Clear</Button>}
    </Stack>
    {error && <p className="settings__error" role="alert">{error}</p>}
  </>;
}

/** The shape materials are previewed on, in the library and the viewer. */
function MaterialShapeSetting() {
  const style = usePreviewPreferences((state) => state.materialStyle);
  const setStyle = usePreviewPreferences((state) => state.setMaterialStyle);
  return <>
    <h4>Material shape</h4>
    <Segmented label="Material preview style" value={style} options={[{ value: "sphere", label: "Sphere" }, { value: "cube", label: "Cube" }]} onChange={setStyle} />
    <p className="settings__description">Thumbnails refresh automatically. Existing images stay visible until their replacements are ready.</p>
  </>;
}
