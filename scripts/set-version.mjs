// Set the one Toybox version everywhere it is written: package.json, src-tauri/tauri.conf.json and
// `[workspace.package] version` of src-tauri/Cargo.toml (which every crate inherits). The app, the
// server and `tbx` are released together and carry the same number.
//
//   node scripts/set-version.mjs 0.5.0
//
// Deliberately untouched: the SDK (`packages/toybox-sdk`, its own semver for plugin authors) and the
// plugin manifests (each plugin has its own version).
import {readFileSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {dirname, join} from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? '')) {
  console.error('Usage: node scripts/set-version.mjs <version>   (for example 0.5.0 or 0.5.0-rc.1)');
  process.exit(2);
}

function edit(path, change) {
  const file = join(root, path);
  const before = readFileSync(file, 'utf8');
  const after = change(before);
  if (after === before) console.log(`${path}: already ${version}`);
  else {
    writeFileSync(file, after);
    console.log(`${path}: ${version}`);
  }
}

// Keep each file's own formatting: only the version value is replaced.
edit('package.json', (text) => text.replace(/^(\s*"version"\s*:\s*)"[^"]*"/m, `$1"${version}"`));
edit('src-tauri/tauri.conf.json', (text) => text.replace(/^(\s*"version"\s*:\s*)"[^"]*"/m, `$1"${version}"`));
edit('src-tauri/Cargo.toml', (text) => {
  if (!/\[workspace\.package\]/.test(text)) throw Error('src-tauri/Cargo.toml has no [workspace.package]');
  return text.replace(/(\[workspace\.package\][^\[]*?\nversion\s*=\s*)"[^"]*"/, `$1"${version}"`);
});

// Cargo.lock lists the workspace crates with their version: let cargo rewrite it, offline.
import {spawnSync} from 'node:child_process';
const lock = spawnSync('cargo', ['update', '--workspace', '--offline'], {cwd: join(root, 'src-tauri'), stdio: 'inherit'});
if (lock.status !== 0) console.warn('Run `cargo update --workspace` in src-tauri to refresh Cargo.lock.');
console.log(`Version ${version} set. Check it with: node scripts/check-release.mjs (needs RELEASE_TAG and the signing key in CI).`);
