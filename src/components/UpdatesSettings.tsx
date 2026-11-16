import { Button } from "@ldlework/toybox-sdk/ui";
import { useAppUpdates } from "../state/appUpdates";

export function UpdatesSettings() {
  const updates=useAppUpdates();
  const busy=["checking","downloading","installing"].includes(updates.status);
  return <>
    <h2>Updates</h2><p className="settings__description">Toybox {updates.currentVersion}</p>
    <section className="settings__section">
      <h3>{updates.version ? `Toybox ${updates.version} is available` : "Application updates"}</h3>
      <p className="settings__description">Toybox checks for new versions automatically. Updates install here and restart the app.</p>
      {updates.notes && <p className="updates__notes">{updates.notes}</p>}
      {updates.status==="current" && <p role="status">You're up to date.</p>}
      {updates.status==="development" && <p role="status">Updates are available in installed release builds.</p>}
      {updates.status==="checking" && <p role="status">Checking for updates…</p>}
      <UpdateProgress/>
      {updates.error && <p className="settings__error" role="alert">{updates.error}</p>}
      <div className="updates__actions">
        <Button disabled={busy} onClick={()=>void updates.check()}>Check for updates</Button>
        {updates.version && <Button variant="primary" disabled={busy} onClick={()=>void updates.install()}>Install and restart</Button>}
      </div>
    </section>
  </>;
}

export function UpdateProgress() {
  const {status,downloaded,total}=useAppUpdates();
  if (status==="installing") return <p role="status">Installing update and restarting…</p>;
  if (status!=="downloading") return null;
  return <div className="updates__progress" role="status"><span>Downloading update… {total?`${Math.min(100,Math.round(downloaded/total*100))}%`:`${(downloaded/1048576).toFixed(1)} MB`}</span>
    <progress aria-label="Update download" max={total??1} value={total?downloaded:undefined}/>
  </div>;
}
