// Screenshots of every tab in light and dark at 1100×760 and 700×600 against
// the mock backend, for before/after comparison. Writes into $SCREENS_DIR
// (default target/ui-screens).
//   node scripts/ui-checks/harness.mjs dist scripts/ui-checks/screens.mjs
import { mkdirSync, readFileSync } from 'node:fs';

const INDEX = JSON.parse(readFileSync(new URL('../../src-ui/fixtures/shortcuts-index.json', import.meta.url)));

const out = process.env.SCREENS_DIR ?? 'target/ui-screens';
const tabs = ['home', 'apps', 'updates', 'shortcuts', 'learn', 'connections', 'engines', 'settings'];
const sizes = [[1100, 760], [700, 600]];

export default {
  async 'every tab, both themes, two sizes'(page) {
    mkdirSync(out, { recursive: true });
    await page.waitForSelector('#tab-shortcuts', { timeout: 10000 });
    await page.evaluate((index) => { window.__mock.on('shortcuts_index', () => index); window.__mock.emit('shortcut-index-built', null); }, INDEX);
    let count = 0;
    for (const theme of ['light', 'dark']) {
      await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
      for (const [width, height] of sizes) {
        await page.setViewportSize({ width, height });
        for (const tab of tabs) {
          await page.click(`#tab-${tab}`);
          await page.waitForSelector(`#panel-${tab}:not([hidden])`, { timeout: 10000 });
          await page.waitForTimeout(400);
          await page.screenshot({ path: `${out}/${tab}-${theme}-${width}x${height}.png` });
          count += 1;
        }
      }
    }
    return `${count} screenshots in ${out}`;
  },
};
