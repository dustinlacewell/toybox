import { create } from "zustand";
import { isTauri } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { pauseForUpdate } from "../services/updateLifecycle";
import app from "../../package.json";

type Status = "idle" | "checking" | "current" | "available" | "downloading" | "installing" | "error" | "development";
let pending: Update | null = null;
export const useAppUpdates = create<{
  currentVersion: string; version?: string; notes?: string; status: Status;
  downloaded: number; total?: number; error?: string; dismissed: boolean;
  check: () => Promise<void>; install: () => Promise<void>; dismiss: () => void;
}>((set, get) => ({
  currentVersion: app.version, status: "idle", downloaded: 0, dismissed: false,
  dismiss: () => set({dismissed:true}),
  check: async () => {
    if (["checking","downloading","installing"].includes(get().status)) return;
    if (!isTauri() || import.meta.env.DEV) { set({status:"development"}); return; }
    set({status:"checking",error:undefined,version:undefined,notes:undefined});
    try {
      const previous=pending;pending=null;await previous?.close().catch(()=>{});
      pending=await check({timeout:15000});
      set(pending ? {status:"available",version:pending.version,notes:pending.body,dismissed:false} : {status:"current",version:undefined,notes:undefined});
    } catch (failure) { set({status:"error",error:String(failure)}); }
  },
  install: async () => {
    if (!pending || !["available","error"].includes(get().status)) return;
    const update=pending;
    let resume:(()=>void)|undefined;
    set({status:"downloading",downloaded:0,total:undefined,error:undefined,dismissed:false});
    try {
      await update.download(event => {
        if (event.event==="Started") set({total:event.data.contentLength,downloaded:0});
        else if (event.event==="Progress") set(state=>({downloaded:state.downloaded+event.data.chunkLength}));
      }, {timeout:120000});
      set({status:"installing"});
      resume=await pauseForUpdate();
      await update.install();
      await relaunch();
    } catch (failure) { resume?.();set({status:"error",error:String(failure)}); }
  },
}));
