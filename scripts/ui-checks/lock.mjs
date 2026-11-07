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
  async 'Home samples usage at most every 10 s (TL-8)'(page) {
    await page.click('#tab-home');
    await sleep(500);
    const count = () => page.evaluate(() => window.__mock.calls.filter(([c]) => c === 'dashboard').length);
    const before = await count();
    await sleep(5000);
    const extra = (await count()) - before;
    assert(extra === 0, `Home asked for ${extra} samples in 5 s`);
  },
  async 'Back to apps returns from the details page'(page) {
    await openDetails(page);
    await page.click('.back-button');
    await sleep(200);
    assert(await page.locator('.catalog-detail').count() === 0, 'still on the details page');
  },
  async 'Escape returns from the details page (TL-3)'(page) {
    await openDetails(page);
    await page.keyboard.press('Escape');
    await sleep(200);
    assert(await page.locator('.catalog-detail').count() === 0, 'Escape left the details page open');
  },
  async 'Escape returns from a guide to the guide list (TL-3)'(page) {
    await page.click('#tab-learn');
    await page.locator('#panel-learn button', { hasText: 'Read' }).first().click();
    await page.waitForSelector('#panel-learn .back-button', { timeout: 5000 });
    await page.keyboard.press('Escape');
    await sleep(200);
    assert(await page.locator('#panel-learn .back-button').count() === 0, 'Escape left the guide open');
  },
  async 'Escape clears a search before anything else (UI-2)'(page) {
    await page.click('#tab-learn');
    await page.fill('#docs-query', 'shortcut');
    await page.focus('#docs-query');
    await page.keyboard.press('Escape');
    await sleep(200);
    assert(await page.inputValue('#docs-query') === '', 'the search kept its text');
  },
  async 'Escape on a top-level page keeps the window as it is'(page) {
    await page.click('#tab-apps');
    await page.keyboard.press('Escape');
    await sleep(200);
    const tab = await page.textContent('[role="tab"][aria-selected="true"]');
    assert(/Apps/.test(tab), `the page changed to ${tab}`);
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
