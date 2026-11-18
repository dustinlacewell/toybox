import { useEffect, useState } from "react";
import { assetUrl } from "../../services/assetUrl";
export function FileThumbnail({ library, file }: { library: string; file: string }) {
  const [url, setUrl] = useState<string>();
  useEffect(() => { let alive = true; setUrl(undefined); void assetUrl(library, file).then((value) => { if (alive) setUrl(value); }); return () => { alive = false; }; }, [library, file]);
  return url ? <img src={url} alt="" /> : null;
}
