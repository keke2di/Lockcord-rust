// CHROME_BIN may point at any Chromium installation. No browser sign-ins or live accounts used.
require('./check-commands.cjs');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const os = require('node:os');
const { execFile } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const cache = path.join(os.homedir(), 'AppData/Local/ms-playwright');
const chrome = process.env.CHROME_BIN || fs.readdirSync(cache).filter((p) => /^chromium-\d+$/.test(p)).sort().reverse()
  .map((p) => path.join(cache, p, 'chrome-win64/chrome.exe')).find((p) => fs.existsSync(p));
if (!chrome) throw new Error('Set CHROME_BIN to a Chromium executable.');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const server = http.createServer((req, res) => {
  const file = path.resolve(root, '.' + new URL(req.url, 'http://localhost').pathname);
  if (!file.startsWith(root + path.sep)) { res.writeHead(403).end(); return; }
  fs.readFile(file, (error, data) => {
    res.writeHead(error ? 404 : 200, { 'Content-Type': mime[path.extname(file)] || 'application/octet-stream' });
    res.end(error ? '' : data);
  });
});
server.listen(0, '127.0.0.1', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'lockcord-ui-test-'));
  execFile(chrome, ['--headless', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
    '--user-data-dir=' + temp, '--window-size=' + (process.env.SMOKE_WINDOW_SIZE || '1240,800'), '--virtual-time-budget=5000', '--dump-dom',
    `http://127.0.0.1:${server.address().port}/tests/ui-smoke.html`], { timeout: 30000, maxBuffer: 4 * 1024 * 1024, windowsHide: true }, (error, stdout, stderr) => {
      const match = stdout.match(/<pre id="test-result">([\s\S]*?)<\/pre>/);
      console.log(match ? match[1].replace(/&gt;/g, '>').replace(/&lt;/g, '<').replace(/&amp;/g, '&') : 'FAIL: browser test did not finish');
      if (!match) console.error(error?.message || stderr.slice(-2000));
      process.exitCode = error || !match || !match[1].startsWith('PASS:') ? 1 : 0;
      server.close();
      fs.rmSync(temp, { recursive: true, force: true, maxRetries: 4, retryDelay: 200 });
    });
});
