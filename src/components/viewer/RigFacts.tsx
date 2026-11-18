import type { SceneAsset } from "../../domain/catalog";
import { useStore } from "../../state/store";

/** Rig inspection belongs to the application; the design system stays generic. */
export function RigFacts({ asset }: { asset: SceneAsset }) {
  const registry = useStore(state => state.rigs);
  const rigs = (asset.rigs ?? []).flatMap(binding => {
    const rig = registry?.find(rig => rig.id === binding.rigId); return rig ? [rig] : [];
  });
  return <>{rigs.map(rig => <details className="viewer-rig" key={rig.id}>
    <summary>{rig.kind === "skeleton" ? "Rig" : "Node hierarchy"} · {rig.nodes.length} nodes</summary>
    {!rig.unambiguous && <p className="viewer-panel-subtitle">Ambiguous node names prevent automatic matching.</p>}
    <ul aria-label="Rig hierarchy">{rig.nodes.map((node,index) => <li key={`${node.key}:${index}`} style={{ paddingLeft: `${Math.min(node.key.split("/").length - 1, 10) * 10}px` }}>{node.name || "Unnamed node"}</li>)}</ul>
  </details>)}</>;
}
