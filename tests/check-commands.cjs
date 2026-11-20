// Every command needs a handler, a build manifest entry and permission for the main window.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const read = (name) => fs.readFileSync(path.join(root, name), 'utf8');
const build = read('src-tauri/build.rs').match(/const COMMANDS: &\[&str\] = &\[([\s\S]*?)\];/);
const handler = read('src-tauri/src/main.rs').match(/tauri::generate_handler!\[([\s\S]*?)\]/);
assert(build && handler, 'Command declarations must be found');
const manifest = [...build[1].matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]);
const commands = handler[1].split(',').map((s) => s.trim()).filter(Boolean);
const capability = JSON.parse(read('src-tauri/capabilities/default.json'));
const permissions = capability.permissions.filter((p) => p.startsWith('allow-'))
  .map((p) => p.slice(6).replaceAll('-', '_'));
assert.deepEqual(capability.windows, ['main'], 'App commands must stay scoped to the main window');
for (const [label, list] of [['manifest', manifest], ['handler', commands], ['permissions', permissions]]) {
  assert.equal(new Set(list).size, list.length, 'Duplicate entries in ' + label);
}
assert.deepEqual([...manifest].sort(), [...commands].sort(), 'Build manifest and command handlers differ');
assert.deepEqual([...permissions].sort(), [...commands].sort(), 'Main-window permissions and command handlers differ');
console.log('PASS: ' + commands.length + ' commands match the build manifest and main-window permissions');
