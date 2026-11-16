/** The application screen itself; only browser data and filesystem adapters differ. */
import { useEffect, useState } from "react";
import { LibraryView } from "@app/views/LibraryView";
import { Button, Dialog } from "@ldlework/toybox-sdk/ui";
import "./demo-app.css";

export default function ToyboxDemo() {
  const [desktopNotice,setDesktopNotice]=useState(false);
  useEffect(()=>{
    const open=()=>setDesktopNotice(true);
    window.addEventListener("toybox-desktop-action",open);
    return()=>window.removeEventListener("toybox-desktop-action",open);
  },[]);
  return <div className="toybox-app">
    <LibraryView/>
    <Dialog open={desktopNotice} size="compact" title="Use your own files in Toybox" onClose={()=>setDesktopNotice(false)}
      footer={<Button onClick={()=>setDesktopNotice(false)}>Back to demo</Button>}>
      <div className="demo-desktop-panel"><p>Browsing, previews, and playback work here. Install Toybox to import files and export to folders on your computer.</p>
        <a href="https://github.com/dustinlacewell/toybox/releases/latest" target="_blank" rel="noreferrer">Download Toybox</a>
      </div>
    </Dialog>
  </div>;
}
