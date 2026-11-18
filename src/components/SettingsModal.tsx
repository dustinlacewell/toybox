import { useState, type ReactNode } from "react";
import { Dialog, CategoryLayout } from "@ldlework/toybox-sdk/ui";
import { Folder, Puzzle, Download, type LucideIcon } from "lucide-react";
import type { PluginLoadError } from "../services/pluginRegistry";
import type { Provided } from "../services/pluginProvides";
import type { PluginManifestDto } from "../services/tauriApi";
import "./SettingsModal.css";
import { PluginsSettings } from "./PluginsSettings";
import { UpdatesSettings } from "./UpdatesSettings";
import { LibrariesSettings } from "./LibrariesSettings";

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
  /** Every installed plugin, on or off. */
  plugins: PluginManifestDto[];
  pluginErrors: PluginLoadError[];
  /** What each loaded plugin provides, by plugin id. */
  pluginProvided: Record<string, Provided>;
  /** Discover plugins again after one is turned on or off. */
  onPluginsChanged: () => Promise<void>;
  categories?: readonly SettingsCategory[];
}

export function SettingsModal({ open, onClose, plugins, pluginErrors, pluginProvided, onPluginsChanged, categories = [] }: Props) {
  const [category, setCategory] = useState("library");

  // Settings of an asset type live under the plugin that provides it, on the
  // Plugins page: the FBX converter with models, the preview shape with materials.
  const sections: SettingsCategory[] = [
    { id: "library", title: "Library", icon: Folder, content: <LibrariesSettings /> },
    { id: "plugins", title: "Plugins", icon: Puzzle, content: <PluginsSettings plugins={plugins} errors={pluginErrors} provided={pluginProvided} onChanged={onPluginsChanged} /> },
    {id:"updates",title:"Updates",icon:Download,content:<UpdatesSettings/>},
    ...categories.filter(section => !["library", "plugins", "updates"].includes(section.id)),
  ];
  const active = sections.find(section => section.id === category) ?? sections[0];
  return <Dialog open={open} onClose={onClose} title="Settings" size="medium">
    <CategoryLayout label="Settings categories" value={active.id} onChange={setCategory}
      options={sections.map(({ id, title, icon: Icon }) => ({ id, label: title, icon: Icon && <Icon size={16} aria-hidden="true"/> }))}>
      <div className="settings__panel">{open && active.content}</div>
    </CategoryLayout>
  </Dialog>;
}
