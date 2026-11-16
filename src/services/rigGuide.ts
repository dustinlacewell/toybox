import { BufferAttribute, BufferGeometry, Group, LineBasicMaterial, LineSegments, Mesh, Object3D, Points, PointsMaterial, Vector3 } from "three";

export function hasVisibleGeometry(root: Object3D): boolean {
  let found = false;
  root.traverse((node) => { if (node instanceof Mesh && node.geometry.attributes.position?.count) found = true; });
  return found;
}

/** Draw authored node/joint positions for animation sources without a visible mesh. */
export function createRigGuide(root: Object3D, size = 1) {
  const nodes: Object3D[] = [];
  root.traverse((node) => { if (node !== root && !(node instanceof Mesh)) nodes.push(node); });
  if (!nodes.length) nodes.push(root);
  const included = new Set(nodes);
  const edges = nodes.filter((node) => node.parent && included.has(node.parent));
  const positions = new Float32Array(nodes.length * 3);
  const links = new Float32Array(edges.length * 6);
  const joints = new BufferGeometry(); joints.setAttribute("position", new BufferAttribute(positions, 3));
  const bones = new BufferGeometry(); bones.setAttribute("position", new BufferAttribute(links, 3));
  const object = new Group();
  object.add(new Points(joints, new PointsMaterial({ color: "#eeeeee", size: Math.max(size * 0.015, 0.008), sizeAttenuation: true, depthTest: false })));
  object.add(new LineSegments(bones, new LineBasicMaterial({ color: "#b4bdce", depthTest: false })));
  const position = new Vector3();
  const update = () => {
    root.updateWorldMatrix(true, true);
    nodes.forEach((node, index) => node.getWorldPosition(position).toArray(positions, index * 3));
    edges.forEach((node, index) => { node.getWorldPosition(position).toArray(links, index * 6); node.parent!.getWorldPosition(position).toArray(links, index * 6 + 3); });
    joints.attributes.position.needsUpdate = true; bones.attributes.position.needsUpdate = true;
    joints.computeBoundingSphere(); bones.computeBoundingSphere();
  };
  update();
  return { object, update };
}
