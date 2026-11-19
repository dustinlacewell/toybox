// Builds tbx for one target and puts it where Tauri's `bundle.externalBin` ("binaries/tbx") expects it:
// src-tauri/binaries/tbx-<target-triple>[.exe]. Release builds only (src-tauri/tauri.release.conf.json);
// `tauri dev` and a plain `tauri build` do not need it.
//
//   node scripts/prepare-tbx-sidecar.mjs [--target <triple>] [--skip-build]
//
// tbx embeds the compiled plugin SDK, so the SDK is built first (unless packages/toybox-sdk/dist is there
// already or TOYBOX_SDK_DIST says where it is).
import {copyFileSync, existsSync, mkdirSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';

const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
const run = (command, argv, options = {}) => {
  const result = spawnSync(command, argv, {stdio: 'inherit', shell: process.platform === 'win32' && !command.endsWith('.exe'), ...options});
  if (result.status !== 0) { console.error(`error: ${command} ${argv.join(' ')} failed`); process.exit(result.status ?? 1); }
};
const host = spawnSync('rustc', ['-vV'], {encoding: 'utf8'}).stdout.match(/host: (\S+)/)?.[1];
const target = flag('--target') ?? process.env.RUST_TARGET ?? host;
if (!target) { console.error('error: cannot tell the target; pass --target <triple>'); process.exit(2); }
const exe = target.includes('windows') ? '.exe' : '';
const targetDir = resolve(process.env.CARGO_TARGET_DIR ?? 'src-tauri/target');

if (!args.includes('--skip-build')) {
  const sdk = process.env.TOYBOX_SDK_DIST ? undefined : resolve('packages/toybox-sdk/dist/index.js');
  if (sdk && !existsSync(sdk)) {
    const tsc = resolve('node_modules/.bin/tsc');
    run(existsSync(tsc) || existsSync(tsc + '.cmd') ? tsc : 'tsc', ['-p', 'packages/toybox-sdk/tsconfig.json']);
  }
  run('cargo', ['build', '--release', '-p', 'toybox-cli', '--target', target], {cwd: 'src-tauri', env: {...process.env, CARGO_TARGET_DIR: targetDir}});
}
const built = join(targetDir, target, 'release', `tbx${exe}`);
if (!existsSync(built)) { console.error(`error: ${built} was not built`); process.exit(1); }
mkdirSync('src-tauri/binaries', {recursive: true});
const placed = join('src-tauri', 'binaries', `tbx-${target}${exe}`);
copyFileSync(built, placed);
console.log(`tbx for ${target} is at ${placed}`);
