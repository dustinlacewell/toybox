import type { AudioAsset } from "../domain/catalog";
export async function audioThumbnail(asset:AudioAsset):Promise<Uint8Array> {
  const canvas=document.createElement("canvas");canvas.width=256;canvas.height=256;
  const ctx=canvas.getContext("2d");if(!ctx)throw Error("Cannot draw audio waveform");
  ctx.fillStyle="#14161a";ctx.fillRect(0,0,256,256);ctx.fillStyle="#a3a3a3";
  const peaks=asset.audio.peaks;
  peaks.forEach((peak,i)=>{const h=Math.max(2,peak*140);ctx.fillRect(20+i*216/peaks.length,128-h/2,Math.max(1,216/peaks.length*.7),h);});
  ctx.font="12px system-ui";ctx.fillStyle="#6f6f6f";ctx.fillText(asset.audio.format,20,230);
  const blob=await new Promise<Blob|null>(resolve=>canvas.toBlob(resolve,"image/png"));if(!blob)throw Error("Cannot capture audio waveform");
  return new Uint8Array(await blob.arrayBuffer());
}
