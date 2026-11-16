import {readFileSync} from 'node:fs';
const app=JSON.parse(readFileSync('package.json','utf8'));
const config=JSON.parse(readFileSync('src-tauri/tauri.conf.json','utf8'));
const rust=readFileSync('src-tauri/Cargo.toml','utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if(app.version!==config.version||rust!==app.version)throw Error('App, Cargo and Tauri versions must match.');
if(process.env.RELEASE_TAG?.replace(/^v/,'')!==app.version)throw Error('Release tag must match the app version.');
if(!process.env.TAURI_SIGNING_PRIVATE_KEY)throw Error('Configure the TAURI_SIGNING_PRIVATE_KEY repository secret before releasing.');
if(!config.plugins?.updater?.pubkey||!config.bundle.createUpdaterArtifacts)throw Error('Signed updater configuration is incomplete.');
console.log(`Release ${app.version}: version and signing configuration present.`);
