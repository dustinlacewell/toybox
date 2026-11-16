import { useEffect, useState } from "react";
import { assetUrl } from "../../services/assetUrl";
export function FileThumbnail({ file }: { file: string }) {
  const [url, setUrl] = useState<string>();
  useEffect(() => { let alive = true; setUrl(undefined); void assetUrl(file).then((value) => { if (alive) setUrl(value); }); return () => { alive = false; }; }, [file]);
  return url ? <img src={url} alt="" /> : null;
}
