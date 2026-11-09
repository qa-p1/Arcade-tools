import { readFileSync } from 'node:fs';
const INDEX = JSON.parse(readFileSync(new URL('../../src-ui/fixtures/shortcuts-index.json', import.meta.url)));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const tab = (page, id) => page.click(`#tab-${id}`);
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const text = (page, sel) => page.textContent(sel).then((t) => t.replace(/\s+/g, ' ').trim());
async function shortcutsPage(page, wmName) {
  await page.evaluate(([index, wm]) => {
    const m = window.__mock;
    m.on('shortcuts_index', () => ({ ...index, wm: { ...index.wm, name: wm } }));
    m.emit('shortcut-index-built', null);
  }, [INDEX, wmName]);
  await sleep(200);
  await tab(page, 'shortcuts');
  // The first rebindable Arcade shortcut; clicking its recorder starts recording.
  const row = await page.locator('.rebind-control').filter({ has: page.locator('.shortcut-recorder') }).first().elementHandle();
  await (await row.$('.shortcut-recorder')).click();
  await sleep(100);
  return row;
}
// Tools' answer to initialize arrives late; the user picks a page meanwhile.
async function userTabSurvivesSlowStart(page) {
  await page.addInitScript(() => {
    const m = window.__mock;
    m.state.preferences.onboardingComplete = false;
    const initialize = () => ({ apps: m.state.apps, preferences: m.state.preferences, updates: m.state.updates, selected: null, diagnostics: [], testing: false });
    m.on('initialize', () => m.later(1500, () => { window.__initialized = true; return initialize(); }));
  });
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.click('#tab-settings');
  await page.waitForFunction(() => window.__initialized, null, { timeout: 10000 });
  await sleep(300);
  const context = await text(page, '[role="tab"][aria-selected="true"]');
  assert(context.endsWith('Settings'), `the window switched pages by itself: ${context}`);
}

export default {
  'A page chosen while Tools starts is kept': userTabSurvivesSlowStart,
  async 'No "none bindings" without a window manager'(page) {
    const row = await shortcutsPage(page, 'none');
    const t = await row.textContent();
    assert(!t.includes('none bindings'), t);
  },
  async 'A global shortcut needs Ctrl, Alt or Super'(page) {
    const row = await shortcutsPage(page, 'hyprland');
    await page.keyboard.press('Shift+KeyS');
    await sleep(100);
    const recorder = await row.$('.shortcut-recorder');
    assert(/Add Ctrl, Alt or Super/.test(await row.textContent()), 'no hint about the missing modifier');
    assert(/Press keys/.test(await recorder.textContent()), 'Shift+S ended recording as if it were a valid shortcut');
    await page.keyboard.press('Control+Alt+KeyS');
    await sleep(200);
    const keys = await recorder.$$eval('.key-chip', (chips) => chips.map((chip) => chip.textContent).join(' '));
    assert(keys === 'Ctrl Alt S', `Ctrl+Alt+S was not applied: "${keys}"`);
  },
  async 'Shortcut clash shows before applying'(page) {
    const row = await shortcutsPage(page, 'hyprland');
    await page.keyboard.press('Meta+KeyQ');
    await sleep(100);
    const t = (await row.textContent()).replace(/\s+/g, ' ');
    assert(/is used by/.test(t) && /Use anyway/.test(t) && /Choose another/.test(t), `no warning: ${t}`);
  },
  async 'Card offers adoption for an app Tools does not manage'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.state.apps = m.state.apps.map((a) => a.id === 'arcade.find' ? { ...a, managed: false, installationClass: 'manual', reason: 'Installed outside Arcade Tools.' } : a);
      m.emit('registry-changed', null);
    });
    await sleep(200);
    await tab(page, 'apps');
    const n = await page.locator('article#arcade\\.find').getByRole('button', { name: 'Let Tools manage it' }).count();
    assert(n === 1, `adopt buttons on card: ${n}`);
  },
  async 'Updates: never checked is not "up to date"'(page) {
    await tab(page, 'updates');
    const t = await text(page, '#panel-updates .empty-state');
    assert(!t.includes('You are up to date') && t.includes('Not checked yet'), t);
  },
  async 'Updates: history and rollback read naturally'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('update_history', () => [
        { time: 1, app: 'arcade.shelf', from: null, to: null, result: 'failed', reason: 'network' },
        { time: 2, app: 'arcade.look', from: '1.0.0', to: '2.0.0', result: 'rolled_back', reason: null },
      ]);
      m.state.apps = m.state.apps.map((a) => a.id === 'arcade.box' ? { ...a, version: '1.0.0', rollbackVersion: '2.0.0' } : a);
      m.emit('registry-changed', null);
    });
    await sleep(200);
    await tab(page, 'updates');
    await sleep(300);
    const history = await text(page, '.update-history');
    const rollback = await text(page, '#panel-updates [aria-labelledby="rollback-heading"]');
    assert(!history.includes('rolled_back') && history.includes('Rolled back') && !history.includes('— → —'), history);
    assert(rollback.includes('Switch to v2.0.0'), rollback);
  },
  async 'Updates: a withdrawn version says why and offers the rollback'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.state.apps = m.state.apps.map((a) => a.id === 'arcade.look'
        ? { ...a, version: '2.0.0', rollbackVersion: '1.0.0', withdrawn: 'Loses your settings on start.' } : a);
      m.emit('registry-changed', null);
    });
    await sleep(200);
    await tab(page, 'updates');
    await sleep(300);
    const section = await text(page, '#panel-updates [aria-labelledby="withdrawn-heading"]');
    assert(section.includes('Loses your settings on start.') && section.includes('Roll back to v1.0.0'), section);
  },
  async 'Nightly: choosing it reviews the switch at once and the card says what is installed (TL-9)'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('set_channel', ({ id, channel }) => {
        m.state.apps = m.state.apps.map((a) => a.id === id ? { ...a, channel } : a);
      });
      m.on('check_release', ({ id }) => ({ id, version: '1.1.0', channel: 'nightly', file: 'x.AppImage', notes: '', updateAvailable: true }));
    });
    await tab(page, 'apps');
    await sleep(200);
    await page.selectOption('select[aria-label="Arcade Look channel"]', 'nightly');
    await sleep(400);
    const dialog = await text(page, '#confirm');
    assert(dialog.includes('Update Arcade Look 1.1.0'), dialog);
    await page.keyboard.press('Escape');
    await sleep(300);
    const card = await text(page, 'article[aria-label="Arcade Look"]');
    const badges = () => page.locator('article[aria-label="Arcade Look"] .channel-badge').count();
    assert(card.includes('Switching to nightly: not installed yet.') && await badges() === 0, card);
    await page.evaluate(() => {
      const m = window.__mock;
      m.state.apps = m.state.apps.map((a) => a.id === 'arcade.look' ? { ...a, installedChannel: 'nightly' } : a);
      m.emit('registry-changed', null);
    });
    await sleep(300);
    const after = await text(page, 'article[aria-label="Arcade Look"]');
    assert(await badges() === 1 && !after.includes('not installed yet'), after);
  },
  async 'Details show notes even when rendering fails'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('render_markdown', () => { throw { code: 'markdown', message: 'bad markdown' }; });
      m.on('check_release', ({ id }) => ({ id, version: '2.0.0', channel: 'stable', file: 'x', notes: 'PLAIN NOTES', updateAvailable: true }));
    });
    await tab(page, 'apps');
    await page.locator('article#arcade\\.shelf').getByRole('button', { name: 'Check for updates' }).click();
    await sleep(200);
    await page.locator('article#arcade\\.shelf').getByRole('button', { name: 'Details' }).click();
    await sleep(400);
    const notes = await text(page, '.catalog-detail .release-notes');
    assert(notes.includes('PLAIN NOTES') && !notes.includes('Preparing'), notes);
  },
  async 'Messages stay in view when the page is scrolled'(page) {
    await tab(page, 'settings');
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await page.evaluate(() => window.__mock.emit('manager-message', 'VISIBLE ERROR'));
    await sleep(200);
    const box = await page.locator('#message').boundingBox();
    assert(box && box.y >= 0 && box.y < 900, `message at ${JSON.stringify(box)}`);
  },
  async 'Cancel is reported plainly, with app names in progress'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('check_release', ({ id }) => ({ id, version: '2.0.0', channel: 'stable', file: 'x', notes: '', updateAvailable: true }));
      m.on('operate', () => new Promise((_, reject) => setTimeout(() => reject({ code: 'cancelled', message: 'Cancelled.' }), 300)));
    });
    await tab(page, 'apps');
    await page.locator('article#arcade\\.look').getByRole('button', { name: 'Check for updates' }).click();
    await sleep(200);
    await page.locator('article#arcade\\.look').getByRole('button', { name: 'Update', exact: true }).click();
    await page.click('#confirm-action');
    await page.evaluate(() => window.__mock.emit('operation-progress', { id: 'arcade.tools', phase: 'Downloading', fraction: 0.5, cancellable: true }));
    await sleep(50);
    const phase = await page.textContent('body');
    await sleep(500);
    const message = await page.locator('#message').textContent();
    const error = await page.locator('#message').getAttribute('class');
    assert(!phase.includes('arcade.tools ·') , 'raw id in progress');
    assert(/cancelled/i.test(message) && !String(error).includes('error'), `${message} (${error})`);
  },
  async 'Tools self-update asks first'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.state.updates = { schema: 1, lastCheck: 1, lastSuccess: 1, lastFailure: null, apps: { 'arcade.tools': { installedVersion: '0.2.1', channel: 'stable', updateAvailable: true, heldBack: false, reason: null, latest: { stable: { latestVersion: '0.3.0', updateAvailable: true, heldBack: false, reason: null, error: null, cache: null } } } } };
      m.emit('updates-changed', m.state.updates);
    });
    await tab(page, 'updates');
    await page.click('.update-card button.primary-action');
    await sleep(300);
    const asked = await page.locator('#confirm[open]').count();
    const updated = await page.evaluate(() => window.__mock.calls.some(([c]) => c === 'self_update'));
    assert(asked === 1 && !updated, `dialog ${asked}, self_update called ${updated}`);
  },
  async 'About shows the bundled licences in a dialog that Escape closes'(page) {
    await page.evaluate(() => window.__mock.on('third_party_notices', () => '# Third-party notices\n\n- svelte 5 (MIT)'));
    await tab(page, 'settings');
    await page.getByRole('button', { name: 'Show licences' }).click();
    await page.waitForSelector('#notices[open]', { timeout: 3000 });
    const shown = await text(page, '#notices pre');
    assert(shown.includes('svelte 5 (MIT)'), `notices dialog shows "${shown}"`);
    await page.keyboard.press('Escape');
    await sleep(200);
    const open = await page.locator('#notices[open]').count();
    const kept = await page.textContent('#notices pre');
    assert(open === 0 && kept === '', `after Escape: open ${open}, text kept ${kept.length} chars`);
  },
  async 'A crash report is read before it can leave the machine (TOOLS.md)'(page) {
    const report = 'app: Arcade Box\nversion: 1.0.0\nkind: panic\nmessage: index out of bounds\n\nbacktrace:\n   0: arcade_box::main\n';
    await page.evaluate((report) => {
      const m = window.__mock;
      m.state.crashes = [{ app: 'arcade.box', file: '00001794190000-41-panic.crash', created: Date.now() / 1000 - 120, kind: 'panic', message: 'index out of bounds' }];
      m.on('crash_reports', () => m.state.crashes);
      m.on('crash_report', () => report);
      m.on('dismiss_crash', ({ file }) => { m.state.crashes = m.state.crashes.filter((c) => c.file !== file); });
      m.on('open_crash_issue', () => null);
    }, report);
    await tab(page, 'settings');
    await tab(page, 'home');
    await page.waitForSelector('.crash-notice', { timeout: 3000 });
    const notice = await text(page, '.crash-notice');
    assert(/Arcade Box crashed/.test(notice) && notice.includes('index out of bounds'), notice);
    assert(await page.locator('.crash-notice', { hasText: 'Open GitHub issue' }).count() === 0, 'the notice offers an issue before the report was read');
    await page.click('.crash-notice >> text=View report');
    await page.waitForSelector('#crash-report[open]', { timeout: 3000 });
    await page.waitForFunction(() => document.querySelector('#crash-report textarea')?.value.includes('arcade_box::main'), null, { timeout: 3000 });
    await page.click('#crash-report >> text=Open GitHub issue');
    await sleep(200);
    const issue = await page.evaluate(() => window.__mock.calls.find(([c]) => c === 'open_crash_issue'));
    assert(issue && issue[1].id === 'arcade.box' && issue[1].file === '00001794190000-41-panic.crash', JSON.stringify(issue));
    await page.keyboard.press('Escape');
    await sleep(200);
    assert(await page.locator('#crash-report[open]').count() === 0, 'Escape left the report open');
    await page.click('.crash-notice >> text=Dismiss');
    await sleep(300);
    assert(await page.locator('.crash-notice').count() === 0, 'the dismissed report is still listed');
  },
  async 'Home shows no crash notice when there are no reports'(page) {
    await tab(page, 'home');
    await sleep(300);
    assert(await page.locator('.crash-notice').count() === 0, 'a crash notice without reports');
    assert(await page.evaluate(() => window.__mock.calls.some(([c]) => c === 'crash_reports')), 'Home never asked for crash reports');
  },
};
