import type { FsApi } from "@ldlework/toybox-sdk";
export const supportedAudio=(name:string)=>/\.(wav|mp3|ogg|flac)$/i.test(name);
export function audioId(pack:string,category:string,file:string) {
  let hash=0xcbf29ce484222325n;for(const c of `${pack}\0${category}\0${file}`){hash=BigInt.asUintN(64,(hash^BigInt(c.codePointAt(0)!))*0x100000001b3n);}
  return `uid://audio_${hash.toString(16)}`;
}
export async function findAudio(fs:Pick<FsApi,"readDir">,source:string) {
  const files:string[]=[];const seen=new Set<string>();
  async function walk(dir:string) {
    if(seen.has(dir))return;seen.add(dir);
    for(const entry of await fs.readDir(source,dir)) {
      if(entry.isDir)await walk(entry.relPath);else if(supportedAudio(entry.name))files.push(entry.relPath.replace(/\\/g,"/"));
    }
  }
  await walk(".");return files.sort((a,b)=>a.localeCompare(b));
}
