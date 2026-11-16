import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { Button, IconButton, Toast } from "@ldlework/toybox-sdk/ui";
import { X } from "lucide-react";
import { useAppUpdates } from "../state/appUpdates";
import { UpdateProgress } from "./UpdatesSettings";
import "./AppUpdateNotice.css";

export function AppUpdateNotice() {
  const updates=useAppUpdates();
  useEffect(()=>{
    if(!isTauri()||import.meta.env.DEV)return;
    const timer=window.setTimeout(()=>void useAppUpdates.getState().check(),5000);
    return()=>window.clearTimeout(timer);
  },[]);
  if (updates.dismissed || !updates.version || !["available","downloading","installing","error"].includes(updates.status)) return null;
  const busy=["downloading","installing"].includes(updates.status);
  return <Toast label="Toybox update" position="top-right">
    <div className="updates__heading"><strong>Toybox {updates.version} is available</strong>{!busy&&<IconButton icon={X} label="Dismiss update notification" onClick={updates.dismiss}/>}</div>
    <UpdateProgress/>
    {updates.error&&<p role="alert" className="settings__error">{updates.error}</p>}
    {!busy&&<Button variant="primary" onClick={()=>void updates.install()}>Install and restart</Button>}
  </Toast>;
}
