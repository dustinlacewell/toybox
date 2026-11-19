// Builds, signs and describes the standalone programs of a release: `toybox-server` and `tbx`.
//
//   node scripts/build-server-release.mjs [--target <triple>] [--out <dir>] [--only server|tbx|all]
//        [--skip-build] [--bin-dir <dir>] [--notes <text>|@<file>] [--pubkey <file|base64>] [--manifest-only]
//
// For one target it writes, into --out (default release-assets/):
//   toybox-server-<target>[.exe]  +  .sig      (the release workflow builds Linux x86_64 and ARM64)
//   tbx-<target>[.exe]            +  .sig
//   toybox-server.json            the manifest `toybox-server update` reads (version, notes, and per target
//                                 the file name and signature of every toybox-server-<target> in --out)
//
// Signing is `tauri signer sign` with TAURI_SIGNING_PRIVATE_KEY (and _PASSWORD), the key that signs the app's
// updater files. Every signature is checked again here against the public key in src-tauri/tauri.conf.json
// (or --pubkey, for a throwaway key in a trial), so a wrong repository secret fails the build instead of
// publishing programs nobody can verify.
//
// --skip-build uses programs already built (in --bin-dir, default src-tauri/target/<target>/release).
// --manifest-only only rewrites the manifest from what is in --out.
// The release workflow calls this; run it by hand with a key of your own to try it off CI.
import {createHash, createPublicKey, verify} from 'node:crypto';
import {copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
const has = (name) => args.includes(name);
const fail = (message) => { console.error('error: ' + message); process.exit(1); };

const app = JSON.parse(readFileSync('package.json', 'utf8'));
const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const version = app.version;
const host = spawnSync('rustc', ['-vV'], {encoding: 'utf8'}).stdout?.match(/host: (\S+)/)?.[1];
const target = flag('--target') ?? process.env.RUST_TARGET ?? host;
if (!target) fail('cannot tell the target; pass --target <triple>');
const exe = target.includes('windows') ? '.exe' : '';
const out = resolve(flag('--out') ?? 'release-assets');
const only = flag('--only') ?? 'all';
const notesArg = flag('--notes');
const notes = notesArg?.startsWith('@') ? readFileSync(notesArg.slice(1), 'utf8').trim() : notesArg;
const targetDir = resolve(process.env.CARGO_TARGET_DIR ?? 'src-tauri/target');
const binDir = resolve(flag('--bin-dir') ?? join(targetDir, target, 'release'));

// ---- signature check (minisign, as Tauri writes it) --------------------------------------------------
const text = (base64) => Buffer.from(base64.trim(), 'base64').toString('utf8');
const minisignBody = (block) => Buffer.from(block.split(/\r?\n/).filter((line) => line && !line.startsWith('untrusted comment:') && !line.startsWith('trusted comment:'))[0], 'base64');
function checkSignature(publicKeyBase64, signatureBase64, data) {
  const key = minisignBody(text(publicKeyBase64));          // "Ed" + key id (8) + key (32)
  const signature = minisignBody(text(signatureBase64));    // "Ed"/"ED" + key id (8) + signature (64)
  const algorithm = signature.subarray(0, 2).toString();
  if (!key.subarray(2, 10).equals(signature.subarray(2, 10))) return 'it was made with a different key than the public key';
  const der = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10, 42)]);
  const message = algorithm === 'ED' ? createHash('blake2b512').update(data).digest() : data;
  return verify(null, message, createPublicKey({key: der, format: 'der', type: 'spki'}), signature.subarray(10, 74)) ? null : 'the signature does not match the file';
}
const publicKey = (() => {
  const given = flag('--pubkey');
  if (!given) return config.plugins?.updater?.pubkey;
  return existsSync(given) ? readFileSync(given, 'utf8').trim() : given;
})();

function sign(file) {
  const cli = resolve('node_modules/@tauri-apps/cli/tauri.js');
  if (!existsSync(cli)) fail('node_modules/@tauri-apps/cli is missing; install the dependencies first');
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY && !process.env.TAURI_SIGNING_PRIVATE_KEY_PATH) fail('set TAURI_SIGNING_PRIVATE_KEY (the key that signs the app updater files)');
  const result = spawnSync(process.execPath, [cli, 'signer', 'sign', file], {encoding: 'utf8', env: {...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? ''}});
  if (result.status !== 0) fail(`signing ${file} failed:\n${result.stderr || result.stdout}`);
  const signature = readFileSync(file + '.sig', 'utf8').trim();
  const problem = checkSignature(publicKey, signature, readFileSync(file));
  if (problem) fail(`${file}: ${problem}. The signing key does not belong to the public key in tauri.conf.json.`);
  return signature;
}

// ---- build and collect ------------------------------------------------------------------------------
mkdirSync(out, {recursive: true});
const programs = [];
if (only === 'all' || only === 'server') programs.push({name: 'toybox-server', crate: 'toybox-server'});
if (only === 'all' || only === 'tbx') programs.push({name: 'tbx', crate: 'toybox-cli'});

if (!has('--manifest-only')) {
  if (!has('--skip-build')) {
    if (programs.some((p) => p.name === 'tbx') && !process.env.TOYBOX_SDK_DIST && !existsSync('packages/toybox-sdk/dist/index.js')) {
      const tsc = resolve('node_modules/.bin/tsc');
      const built = spawnSync(existsSync(tsc + '.cmd') ? tsc + '.cmd' : tsc, ['-p', 'packages/toybox-sdk/tsconfig.json'], {stdio: 'inherit', shell: process.platform === 'win32'});
      if (built.status !== 0) fail('the plugin SDK did not build (tbx embeds it)');
    }
    const cargoArgs = ['build', '--release', '--target', target, ...programs.flatMap((p) => ['-p', p.crate])];
    const built = spawnSync('cargo', cargoArgs, {stdio: 'inherit', cwd: 'src-tauri', env: {...process.env, CARGO_TARGET_DIR: targetDir}});
    if (built.status !== 0) fail('cargo build failed');
  }
  for (const program of programs) {
    const from = join(binDir, program.name + exe);
    if (!existsSync(from)) fail(`${from} is missing`);
    const to = join(out, `${program.name}-${target}${exe}`);
    copyFileSync(from, to);
    sign(to);
    console.log(`signed ${to}`);
  }
}

// ---- the manifest ------------------------------------------------------------------------------------
const targets = {};
for (const name of readdirSync(out).sort()) {
  const match = name.match(/^toybox-server-(.+?)(\.exe)?$/);
  if (!match || name.endsWith('.sig') || name.endsWith('.json')) continue;
  const signatureFile = join(out, name + '.sig');
  if (!existsSync(signatureFile)) fail(`${name} has no ${name}.sig`);
  const signature = readFileSync(signatureFile, 'utf8').trim();
  const problem = checkSignature(publicKey, signature, readFileSync(join(out, name)));
  if (problem) fail(`${name}: ${problem}`);
  targets[match[1]] = {url: name, signature};
}
const manifest = {version, ...(notes ? {notes} : {}), targets};
writeFileSync(join(out, 'toybox-server.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(`toybox-server.json: version ${version}, targets: ${Object.keys(targets).join(', ') || '(none)'}`);
