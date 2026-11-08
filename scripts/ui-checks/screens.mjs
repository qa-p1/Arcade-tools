// Screenshots of every tab in light and dark at 1100×760 and 700×600 against
// the mock backend, for before/after comparison. Writes into $SCREENS_DIR
// (default target/ui-screens).
//   node scripts/ui-checks/harness.mjs dist scripts/ui-checks/screens.mjs
// With SCREENS_DOCS=1 it writes the user guide's and README's images instead
// (docs/user/images/<tab>.png, light, 1100×760). Retake them after each version
// bump: the sidebar shows the version being built.
import { mkdirSync, readFileSync } from 'node:fs';

const INDEX = JSON.parse(readFileSync(new URL('../../src-ui/fixtures/shortcuts-index.json', import.meta.url)));

const docs = Boolean(process.env.SCREENS_DOCS);
const out = docs ? 'docs/user/images' : (process.env.SCREENS_DIR ?? 'target/ui-screens');
const tabs = docs ? ['home', 'apps', 'updates'] : ['home', 'apps', 'updates', 'shortcuts', 'learn', 'connections', 'engines', 'settings'];
const themes = docs ? ['light'] : ['light', 'dark'];
const sizes = docs ? [[1100, 760]] : [[1100, 760], [700, 600]];

export default {
  async 'every tab, both themes, two sizes'(page) {
    mkdirSync(out, { recursive: true });
    await page.waitForSelector('#tab-shortcuts', { timeout: 10000 });
    await page.evaluate((index) => { window.__mock.on('shortcuts_index', () => index); window.__mock.emit('shortcut-index-built', null); }, INDEX);
    let count = 0;
    for (const theme of themes) {
      await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
      for (const [width, height] of sizes) {
        await page.setViewportSize({ width, height });
        for (const tab of tabs) {
          await page.click(`#tab-${tab}`);
          await page.waitForSelector(`#panel-${tab}:not([hidden])`, { timeout: 10000 });
          await page.waitForTimeout(400);
          await page.screenshot({ path: docs ? `${out}/${tab}.png` : `${out}/${tab}-${theme}-${width}x${height}.png` });
          count += 1;
        }
      }
    }
    return `${count} screenshots in ${out}`;
  },
};
