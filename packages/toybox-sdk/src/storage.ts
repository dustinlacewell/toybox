/** Private staging namespace. Source titles and categories remain metadata;
 * the host publishes these temporary files into managed storage on commit. */
export function importNamespace(source: string): string {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(source.replace(/\\/g, "/"))) {
    hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  }
  return `_import_${hash.toString(16)}`;
}
