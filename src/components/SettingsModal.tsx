import { useEffect, useState, type ReactNode } from "react";
import { Button, Stack, Segmented, Dialog, CategoryLayout } from "@ldlework/toybox-sdk/ui";
import { Folder, Images, FileBox, Download, type LucideIcon } from "lucide-react";
import { usePreviewPreferences } from "../state/previewPreferences";
import { getFbx2gltfPath, setFbx2gltfPath } from "../services/tauriApi";
import { pickFile } from "../services/pickFile";
import "./SettingsModal.css";
import { UpdatesSettings } from "./UpdatesSettings";

/** Additional settings sections use the same navigation and content layout. */
export interface SettingsCategory {
  id: string;
  title: string;
  icon?: LucideIcon;
  content: ReactNode;
}
interface Props {
  open: boolean;
  onClose: () => void;
  libraryRoot: string | null;
  onChangeLibrary: () => void;
  categories?: readonly SettingsCategory[];
}

export function SettingsModal({ open, onClose, libraryRoot, onChangeLibrary, categories = [] }: Props) {
  const materialStyle = usePreviewPreferences(state => state.materialStyle);
  const setMaterialStyle = usePreviewPreferences(state => state.setMaterialStyle);
  const [category, setCategory] = useState("previews");
  const [fbxPath, setFbxPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!open) return;
    let alive = true;
    setLoading(true); setError(undefined);
    void getFbx2gltfPath().then(path => { if (alive) setFbxPath(path); })
      .catch(failure => { if (alive) setError(String(failure)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [open]);

  const changeExe = async (clear = false) => {
    setError(undefined);
    try {
      const chosen = clear ? null : await pickFile();
      if (!clear && !chosen) return;
      await setFbx2gltfPath(chosen);
      setFbxPath(chosen);
    } catch (failure) { setError(String(failure)); }
  };

  const sections: SettingsCategory[] = [
    { id: "library", title: "Library", icon: Folder, content: <>
      <h2>Library</h2><p className="settings__description">Choose the asset library Toybox uses.</p>
      <section className="settings__section"><h3>Library folder</h3>
        <p className="settings__path">{libraryRoot ?? "No library configured"}</p>
        <Button onClick={onChangeLibrary}>Change library…</Button>
      </section>
    </> },
    { id: "previews", title: "Previews", icon: Images, content: <>
      <h2>Previews</h2><p className="settings__description">Set how materials appear in the library and viewer.</p>
      <section className="settings__section"><h3>Material shape</h3>
        <Segmented label="Material preview style" value={materialStyle} options={[{ value: "sphere", label: "Sphere" }, { value: "cube", label: "Cube" }]} onChange={setMaterialStyle}/>
        <p className="settings__description">Thumbnails refresh automatically. Existing images stay visible until their replacements are ready.</p>
      </section>
    </> },
    { id: "importing", title: "Importing", icon: FileBox, content: <>
      <h2>Importing</h2><p className="settings__description">Configure tools used to bring assets into your library.</p>
      <section className="settings__section"><h3>FBX converter</h3>
        <p className="settings__description">Choose an FBX2glTF executable to enable FBX imports.</p>
        <p className="settings__path">{loading ? "Loading…" : fbxPath ?? "No converter selected"}</p>
        <Stack dir="row" gap={8}><Button disabled={loading} onClick={() => void changeExe()}>Choose FBX2glTF…</Button>
          {fbxPath && <Button variant="ghost" disabled={loading} onClick={() => void changeExe(true)}>Clear</Button>}
        </Stack>
        {error && <p className="settings__error" role="alert">{error}</p>}
      </section>
    </> },
    {id:"updates",title:"Updates",icon:Download,content:<UpdatesSettings/>},
    ...categories.filter(section => !["library", "previews", "importing", "updates"].includes(section.id)),
  ];
  const active = sections.find(section => section.id === category) ?? sections[0];
  return <Dialog open={open} onClose={onClose} title="Settings" size="medium">
    <CategoryLayout label="Settings categories" value={active.id} onChange={setCategory}
      options={sections.map(({ id, title, icon: Icon }) => ({ id, label: title, icon: Icon && <Icon size={16} aria-hidden="true"/> }))}>
      <div className="settings__panel">{active.content}</div>
    </CategoryLayout>
  </Dialog>;
}
