/**
 * Application settings, in a right-side drawer. Two concerns live here: the
 * active library (re-point delegates up to the view's picker flow) and the
 * FBX2glTF converter path the importer shells for `.fbx` sources.
 */

import { useEffect, useState } from "react";

import { Button, Drawer, Stack } from "@ldlework/toybox-sdk/ui";
import { getFbx2gltfPath, setFbx2gltfPath } from "../services/tauriApi";
import { pickFile } from "../services/pickFile";
import "./ExportDrawer.css";

interface Props {
  open: boolean;
  onClose: () => void;
  libraryRoot: string | null;
  /** Re-point at a different library — opens the view's library picker. */
  onChangeLibrary: () => void;
}

export function SettingsModal({ open, onClose, libraryRoot, onChangeLibrary }: Props) {
  const [fbxPath, setFbxPath] = useState<string | null>(null);

  useEffect(() => {
    if (open) void getFbx2gltfPath().then(setFbxPath);
  }, [open]);

  const chooseExe = async () => {
    const chosen = await pickFile();
    if (!chosen) return;
    await setFbx2gltfPath(chosen);
    setFbxPath(chosen);
  };

  const clearExe = async () => {
    await setFbx2gltfPath(null);
    setFbxPath(null);
  };

  return (
    <Drawer open={open} onClose={onClose}>
      <div className="export">
        <header className="export__header">
          <h2>Settings</h2>
          <button className="export__close" onClick={onClose}>✕</button>
        </header>

        <section className="export__section">
          <div className="export__label">Library</div>
          <Stack dir="row" gap={8} align="center">
            <Button onClick={onChangeLibrary}>Change library…</Button>
            <span className="export__path" title={libraryRoot ?? ""}>
              {libraryRoot ?? "No library configured"}
            </span>
          </Stack>
        </section>

        <section className="export__section">
          <div className="export__label">FBX converter (FBX2glTF)</div>
          <Stack dir="row" gap={8} align="center">
            <Button onClick={() => void chooseExe()}>Choose FBX2glTF…</Button>
            {fbxPath && <Button variant="ghost" onClick={() => void clearExe()}>Clear</Button>}
            <span className="export__path" title={fbxPath ?? ""}>
              {fbxPath ?? "Not set — .fbx import disabled"}
            </span>
          </Stack>
          <div className="export__hint">
            Point at your FBX2glTF executable to import .fbx files. Without it,
            .fbx sources are skipped.
          </div>
        </section>
      </div>
    </Drawer>
  );
}
