import {existsSync, readFileSync, readdirSync} from 'node:fs';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';

const app=JSON.parse(readFileSync('package.json','utf8'));
const config=JSON.parse(readFileSync('src-tauri/tauri.conf.json','utf8'));
const cargo=readFileSync('src-tauri/Cargo.toml','utf8');
// The one version: [workspace.package] in src-tauri/Cargo.toml, inherited by every crate.
const rust=cargo.match(/\[workspace\.package\][^\[]*?\nversion\s*=\s*"([^"]+)"/)?.[1];
const problems=[];
if(app.version!==config.version||rust!==app.version)problems.push(`package.json ${app.version}, tauri.conf.json ${config.version} and Cargo.toml [workspace.package] ${rust} must be the same version (node scripts/set-version.mjs <version>).`);
// Every crate takes it from the workspace; a crate with a number of its own would break "server and client carry the same version".
const crates=join('src-tauri','crates');
const manifests=['src-tauri/Cargo.toml',...(existsSync(crates)?readdirSync(crates).map(name=>join(crates,name,'Cargo.toml')).filter(existsSync):[])];
for(const path of manifests){
  const text=readFileSync(path,'utf8');
  const package_=text.match(/\[package\]([\s\S]*?)(\n\[|$)/)?.[1]??'';
  if(!/^version\.workspace\s*=\s*true/m.test(package_))problems.push(`${path} must have "version.workspace = true" in [package].`);
}
const lock=readFileSync('src-tauri/Cargo.lock','utf8');
for(const name of ['toybox-client','toybox-core','toybox-host','toybox-remote','toybox-server','toybox-script','toybox-cli']){
  const found=lock.match(new RegExp(String.raw`\[\[package\]\]\r?\nname = "${name}"\r?\nversion = "([^"]+)"`))?.[1];
  if(found&&found!==app.version)problems.push(`Cargo.lock lists ${name} ${found}; run cargo update --workspace (or set-version.mjs).`);
}
const tauri=lock.match(/\[\[package\]\]\r?\nname = "tauri"\r?\nversion = "([^"]+)"/)?.[1];
const api=JSON.parse(readFileSync('node_modules/@tauri-apps/api/package.json','utf8')).version;
if(!tauri||tauri.split('.').slice(0,2).join('.')!==api.split('.').slice(0,2).join('.'))problems.push('Tauri Rust core and JavaScript API must use matching major/minor versions.');
if(!config.plugins?.updater?.pubkey||!config.bundle.createUpdaterArtifacts)problems.push('Signed updater configuration is incomplete.');
if(process.env.RELEASE_TAG&&process.env.RELEASE_TAG.replace(/^v/,'')!==app.version)problems.push('Release tag must match the app version.');
// The signing key and the tag only exist in the release workflow.
// The post-build check (--bundle) only looks at what was built; the key was checked before the build.
if(process.env.CI&&!process.argv.includes('--bundle')&&!process.env.TAURI_SIGNING_PRIVATE_KEY)problems.push('Configure the TAURI_SIGNING_PRIVATE_KEY repository secret before releasing.');
// deploy/install.sh verifies --release downloads with its own copy of the release public key and repository.
const installer=readFileSync('deploy/install.sh','utf8');
if(installer.match(/^RELEASE_PUBKEY="([^"]+)"/m)?.[1]!==config.plugins?.updater?.pubkey)problems.push('deploy/install.sh RELEASE_PUBKEY must equal plugins.updater.pubkey in tauri.conf.json.');
const repository=config.plugins?.updater?.endpoints?.[0]?.match(/github\.com\/([^/]+\/[^/]+)\/releases\//)?.[1];
if(installer.match(/^RELEASE_REPO="([^"]+)"/m)?.[1]!==repository)problems.push(`deploy/install.sh RELEASE_REPO must be ${repository} (the updater endpoint's repository).`);
// The release build ships tbx inside the installers (Tauri's externalBin, from the release-only config).
const triple=process.env.RUST_TARGET||spawnSync('rustc',['-vV'],{encoding:'utf8'}).stdout?.match(/host: (\S+)/)?.[1]||'';
const exe=triple.includes('windows')?'.exe':'';
const release=existsSync('src-tauri/tauri.release.conf.json')?JSON.parse(readFileSync('src-tauri/tauri.release.conf.json','utf8')):{};
if(!(release.bundle?.externalBin??[]).includes('binaries/tbx'))problems.push('src-tauri/tauri.release.conf.json must list "binaries/tbx" in bundle.externalBin.');
if((config.bundle?.externalBin??[]).length)problems.push('tauri.conf.json must not require the tbx sidecar: that belongs in tauri.release.conf.json, so `tauri dev` and a plain build keep working.');
if(process.env.REQUIRE_TBX){
  const sidecar=`src-tauri/binaries/tbx-${triple}${exe}`;
  if(!existsSync(sidecar))problems.push(`${sidecar} is missing: build tbx for ${triple} and place it there before the release build (scripts/prepare-tbx-sidecar.mjs).`);
}
if(process.argv.includes('--bundle')){
  // After the build: tbx must be in what is shipped.
  const out=join('src-tauri','target',...(process.env.BUNDLE_TARGET?[process.env.BUNDLE_TARGET]:[]),'release');
  if(!existsSync(join(out,`tbx${exe}`)))problems.push(`The release build has no tbx${exe} next to the app program (${out}).`);
  const bundle=join(out,'bundle');
  const find=(dir,test)=>{ if(!existsSync(dir))return []; return readdirSync(dir,{withFileTypes:true}).flatMap(e=>{const p=join(dir,e.name);return test(e,p)?[p]:e.isDirectory()?find(p,test):[];}); };
  for(const app of find(bundle,(e,p)=>e.isDirectory()&&p.endsWith('.app')))if(!existsSync(join(app,'Contents','MacOS','tbx')))problems.push(`${app} has no Contents/MacOS/tbx.`);
  for(const deb of find(bundle,(e,p)=>e.isFile()&&p.endsWith('.deb'))){
    const listing=spawnSync('dpkg-deb',['-c',deb],{encoding:'utf8'});
    if(listing.status===0&&!/(^|[\s/])usr\/bin\/tbx\s*$/m.test(listing.stdout))problems.push(`${deb} does not install /usr/bin/tbx.`);
  }
}
if(problems.length){for(const problem of problems)console.error('error: '+problem);process.exit(1);}
console.log(`Release ${app.version}: versions agree${process.env.RELEASE_TAG?', tag matches':''}${process.env.TAURI_SIGNING_PRIVATE_KEY?', signing key present':''}.`);
