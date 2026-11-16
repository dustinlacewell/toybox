import {readFileSync} from 'node:fs';
import {homedir} from 'node:os';
import {join,resolve} from 'node:path';
import {spawnSync} from 'node:child_process';

const path=resolve(process.argv[2]??join(homedir(),'.tauri','toybox-updater.key'));
const key=readFileSync(path,'utf8').trim();
const publicKey=readFileSync(path+'.pub','utf8').trim();
const config=JSON.parse(readFileSync('src-tauri/tauri.conf.json','utf8'));
if(config.plugins?.updater?.pubkey!==publicKey)throw Error('The signing key public-key file must match tauri.conf.json.');
if(!/^[A-Za-z0-9+/]+={0,2}$/.test(key))throw Error('Expected a Tauri base64 private key.');
// Pass exact bytes over stdin. PowerShell pipelines append a newline that
// Tauri's strict base64 decoder rejects; never put the key in process arguments.
const result=spawnSync('gh',['secret','set','TAURI_SIGNING_PRIVATE_KEY','--repo','dustinlacewell/toybox'],{input:key,encoding:'utf8',windowsHide:true});
if(result.status!==0)throw result.error??Error(result.stderr||'GitHub signing-secret setup failed.');
console.log('Configured the Toybox Actions signing secret. The private key remains outside the repository.');
