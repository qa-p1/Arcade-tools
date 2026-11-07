// Behaviour lock for UI findings (docs/v1/audit/tools.md). A scenario named "KNOWN BUG <id>: ..." must FAIL while
// the bug is there; the harness reports XFAIL, and XPASS (a failure) once it is fixed, so the name has to change
// with the fix (docs/v1/DECISIONS.md D3).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (ok, message) => { if (!ok) throw new Error(message); };

async function openDetails(page) {
  await page.click('#tab-apps');
  await page.locator('.app-card .title-button').first().click();
  await page.waitForSelector('.catalog-detail', { timeout: 5000 });
}

export default {
  async 'Back to apps returns from the details page'(page) {
    await openDetails(page);
    await page.click('.back-button');
    await sleep(200);
    assert(await page.locator('.catalog-detail').count() === 0, 'still on the details page');
  },
  async 'KNOWN BUG TL-3: Escape returns from the details page'(page) {
    await openDetails(page);
    await page.keyboard.press('Escape');
    await sleep(200);
    assert(await page.locator('.catalog-detail').count() === 0, 'Escape left the details page open');
  },
  async 'KNOWN BUG TL-10: About names the channel of a self-installed Tools'(page) {
    await page.addInitScript(() => {
      const m = window.__mock;
      m.on('tools_app', () => ({ ...m.state.apps[0], id: 'arcade.tools', name: 'Arcade Tools', version: '0.4.6', managed: false, installationClass: 'selfInstalled', channel: 'stable' }));
    });
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => window.__mock.calls.some(([c]) => c === 'tray_ui_intent'), null, { timeout: 10000 });
    await page.click('#tab-settings');
    await sleep(300);
    const about = (await page.locator('#about-heading').locator('xpath=../..').textContent()).replace(/\s+/g, ' ');
    assert(about.includes('Stable'), `About says: ${about.slice(0, 160)}`);
  },
};
