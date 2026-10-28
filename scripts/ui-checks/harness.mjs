// Load the built Tools UI (dist/) in Chromium against mock.js.
// Usage: npm run build && node scripts/ui-checks/harness.mjs dist scripts/ui-checks/races.mjs
import { createRequire } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
// Playwright is a dev dependency (npm ci); PLAYWRIGHT_MODULES points at
// another install (its node_modules) instead.
const require = createRequire(process.env.PLAYWRIGHT_MODULES ? `${process.env.PLAYWRIGHT_MODULES}/` : import.meta.url);
const { chromium } = require('playwright');

const [dist, scenarioPath] = process.argv.slice(2);
const here = new URL('.', import.meta.url).pathname;
const mock = readFileSync(join(here, 'mock.js'), 'utf8')
  .replace('__CATALOG__', readFileSync(join(here, '../../resources/catalog-snapshot/catalog.json'), 'utf8'))
  .replace('__ENGINES__', readFileSync(join(here, '../../resources/catalog-snapshot/engines.json'), 'utf8'));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.json': 'application/json', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  let file = join(resolve(dist), decodeURIComponent(new URL(req.url, 'http://x').pathname));
  if (!existsSync(file) || file.endsWith('/')) file = join(resolve(dist), 'index.html');
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' });
  res.end(readFileSync(file));
}).listen(0);
const port = server.address().port;
const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const results = [];
try {
  const scenarios = (await import(pathToFileURL(resolve(scenarioPath)))).default;
  for (const [name, run] of Object.entries(scenarios)) {
    const page = await browser.newPage({ viewport: { width: 1100, height: 900 } });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.addInitScript(mock);
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForFunction(() => window.__mock.calls.some(([c]) => c === 'tray_ui_intent'), null, { timeout: 10000 });
    try {
      const detail = await run(page);
      results.push([name, 'PASS', detail ?? '']);
    } catch (error) {
      results.push([name, 'FAIL', error.message.split('\n')[0]]);
    }
    if (errors.length) results.push([name, 'PAGE-ERRORS', errors.join(' | ')]);
    await page.close();
  }
} finally {
  await browser.close();
  server.close();
}
for (const [name, status, detail] of results) console.log(`${status} ${name}${detail ? `: ${detail}` : ''}`);
process.exit(results.some(([, s]) => s !== 'PASS') ? 1 : 0);
