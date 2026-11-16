import { Button } from "@ldlework/toybox-sdk/ui";
import "./PluginPicker.css";

interface Choice {id:string;name:string;description?:string;}

/** Application adapter: controls receive labels and callbacks, never manifests. */
export function PluginPicker({purpose,plugins,onChoose,setups=[],onSetup}:{
  purpose:"import"|"export";plugins:Choice[];onChoose:(id:string)=>void;
  setups?:Choice[];onSetup?:(id:string)=>void;
}) {
  return <section className="plugin-picker" aria-label={`Choose ${purpose} plugin`}>
    <h2>Choose an {purpose} plugin</h2>
    <p>Only plugins that support this type are shown.</p>
    {setups.length>0&&<><h3>Saved exports</h3><div className="plugin-picker__choices">{setups.map(setup=><Button key={setup.id} className="plugin-picker__choice" onClick={()=>onSetup?.(setup.id)}><strong>{setup.name}</strong><small>{setup.description}</small></Button>)}</div><h3>Export plugins</h3></>}
    <div className="plugin-picker__choices">{plugins.map(plugin=><Button key={plugin.id} className="plugin-picker__choice" onClick={()=>onChoose(plugin.id)}><strong>{plugin.name}</strong>{plugin.description&&<small>{plugin.description}</small>}</Button>)}</div>
    {!plugins.length&&<p>No {purpose} plugin is available for this type.</p>}
  </section>;
}
