/** Resolve glTF resource URIs by full library path, preserving atlas identity. */
export function gltfResourcePath(gltfPath: string, uri: string): string | null {
  if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(uri)) return null;
  let decoded: string;
  try { decoded = decodeURIComponent(uri); } catch { decoded = uri; }
  const parent = gltfPath.replace(/\\/g, "/").split("/").slice(0, -1);
  const parts: string[] = [];
  for (const part of [...parent, ...decoded.replace(/\\/g, "/").split("/")]) {
    if (!part || part === ".") continue;
    if (part === "..") { if (!parts.length) return null; parts.pop(); }
    else parts.push(part);
  }
  return parts.join("/");
}
