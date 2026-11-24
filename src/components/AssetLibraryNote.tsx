import type { Asset } from "../domain/catalog";
import { useStore } from "../state/store";
import { useAlsoIn, useOtherVersions } from "../state/duplicates";
import { RemoteMark } from "./RemoteMark";

/** Where the item is: its library, the others holding the same file, and those holding a different version. */
export function AssetLibraryNote({ asset }: { asset: Asset }) {
  const libraries = useStore((state) => state.libraries);
  const owner = libraries.find((library) => library.id === asset.library);
  const elsewhere = useAlsoIn(asset).map((copy) => libraries.find((library) => library.id === copy.library)).filter((library) => !!library);
  const different = useOtherVersions(asset).map((copy) => libraries.find((library) => library.id === copy.library)).filter((library) => !!library);
  return <>{owner?.name ?? asset.library}<RemoteMark library={owner} />
    {!!elsewhere.length && <> · also in {elsewhere.map((library, index) => <span key={library!.id}>{index > 0 && ", "}{library!.name}<RemoteMark library={library} /></span>)}</>}
    {!!different.length && <> · a different version is in {different.map((library, index) => <span key={library!.id}>{index > 0 && ", "}{library!.name}<RemoteMark library={library} /></span>)}</>}
  </>;
}
