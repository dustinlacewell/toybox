import { Cloud } from "lucide-react";

import type { LibraryInfo } from "../domain/catalog";
import "./RemoteLibrary.css";

/** A small cloud next to the name of a library that lives on a server. Nothing for a local one. */
export function RemoteMark({ library }: { library?: Pick<LibraryInfo, "kind"> }) {
  if (library?.kind !== "remote") return null;
  return <span className="remote-mark" role="img" aria-label="Remote library" title="Remote library"><Cloud size={12} aria-hidden="true" /></span>;
}
