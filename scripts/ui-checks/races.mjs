// Group 3 Tools UI checks. Each scenario asserts the correct behaviour.
const tab = (page, id) => page.click(`#tab-${id}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const calls = (page, cmd) => page.evaluate((c) => window.__mock.calls.filter(([x]) => x === c).map(([, a]) => a), cmd);
const assert = (ok, message) => { if (!ok) throw new Error(message); };

async function withUpdate(page, id, version, notes) {
  await page.evaluate(([id, version, notes]) => {
    const m = window.__mock;
    m.state.updates = {
      schema: 1, lastCheck: Math.floor(Date.now() / 1000), lastSuccess: null, lastFailure: null,
      apps: { [id]: { installedVersion: '1.0.0', channel: 'stable', updateAvailable: true, heldBack: false, reason: null,
        latest: { stable: { latestVersion: version, updateAvailable: true, heldBack: false, reason: null, error: null,
          cache: { body: '', release: { version, notes, channel: 'stable' } } } } } },
    };
    m.emit('updates-changed', m.state.updates);
  }, [id, version, notes]);
}

export default {
  async 'N13 Learn search keeps the latest query'(page) {
    await page.evaluate(() => window.__mock.on('search_docs', ({ query }) => window.__mock.later(query === 'keyboard' ? 600 : 40,
      [{ app: 'arcade.find', version: '1', path: `${query}.md`, title: `HIT:${query}`, snippet: '', score: 1 }])));
    await tab(page, 'learn');
    await page.fill('#docs-query', 'keyboard');
    await page.press('#docs-query', 'Enter');
    await sleep(60);
    await page.fill('#docs-query', 'clipboard');
    await page.press('#docs-query', 'Enter');
    await sleep(1000);
    const text = await page.textContent('.docs-results');
    assert(text.includes('HIT:clipboard') && !text.includes('HIT:keyboard'), `results show: ${text.trim().slice(0, 80)}`);
  },
  async 'N14 Learn reader shows the guide of the app last opened'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('docs_pages', ({ id }) => m.later(id === 'arcade.box' ? 600 : 30, [{ path: `${id}/index.md`, title: `${id} intro` }]));
      m.on('read_doc', ({ id, path }) => m.later(id === 'arcade.box' ? 300 : 30, { app: id, version: id === 'arcade.box' ? '9.9.9' : '1.0.0', path, title: path, html: `<p>BODY:${id}</p>`, source: 'bundled', needsFetch: false }));
    });
    await tab(page, 'learn');
    await page.waitForSelector('.learn-app-row');
    const rows = page.locator('.learn-app-row');
    await rows.filter({ hasText: 'Arcade Box' }).getByRole('button', { name: 'Read here' }).click();
    await sleep(50);
    // The reader replaces the list: go back and open another guide while Box's pages are still loading.
    await page.click('text=← All guides');
    await rows.filter({ hasText: 'Arcade Find' }).getByRole('button', { name: 'Read here' }).click();
    await sleep(1300);
    const heading = await page.textContent('#learn-reader-heading');
    const body = await page.textContent('.docs-reader .markdown-body');
    const nav = await page.textContent('.docs-reader nav');
    assert(heading.includes('Find') && body.includes('BODY:arcade.find') && nav.includes('arcade.find'), `heading "${heading}" body "${body}" nav "${nav}"`);
  },
  async 'N15 Release notes follow the current release'(page) {
    await withUpdate(page, 'arcade.look', '2.0.0', 'NOTES FOR TWO');
    await tab(page, 'updates');
    await page.click('text=Show release notes');
    await page.waitForSelector('text=NOTES FOR TWO');
    await withUpdate(page, 'arcade.look', '3.0.0', 'NOTES FOR THREE');
    await sleep(300);
    const card = await page.textContent('.update-card');
    assert(!card.includes('NOTES FOR TWO'), `card still shows v2 notes: ${card.replace(/\s+/g, ' ').slice(0, 160)}`);
  },
  async 'N23 Uninstall dialog matches the plan it submits'(page) {
    await page.evaluate(() => window.__mock.on('plan_uninstall_all', ({ removeData }) => window.__mock.later(500, {
      schema: 1, fingerprint: 'f', removeData, apps: [{ id: 'arcade.look', version: '1.0.0', canUninstall: true, requiresSelfShutdown: false, folders: [], dataFolders: ['/home/u/.config/arcade-look'], reason: null }] })));
    await tab(page, 'settings');
    const box = page.getByLabel('Also delete settings and data');
    await box.check();
    await page.click('text=Uninstall Tools and apps…');
    await sleep(100);
    await box.uncheck().catch(() => {});
    await page.waitForSelector('#confirm[open]');
    const copy = await page.textContent('#confirm-copy');
    await page.click('#confirm-action');
    await sleep(200);
    const [submitted] = await calls(page, 'uninstall_all');
    const says = copy.includes('deleted too');
    assert(submitted && says === submitted.plan.removeData, `dialog says ${says ? 'delete' : 'keep'}, plan removeData=${submitted?.plan.removeData}`);
  },
  async 'N37 An update runs on the channel and version that were reviewed'(page) {
    await withUpdate(page, 'arcade.look', '2.0.0', 'notes');
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('check_release', ({ id }) => m.later(600, { id, version: '2.0.0', channel: 'stable', file: 'x', notes: '', updateAvailable: true }));
      m.on('set_channel', ({ id, channel }) => { m.state.apps = m.state.apps.map((a) => a.id === id ? { ...a, channel } : a); return null; });
      m.on('operate', () => 'done');
    });
    await tab(page, 'updates');
    await page.click('.update-card button.primary-action');
    await tab(page, 'apps');
    await page.selectOption('select[aria-label="Arcade Look channel"]', 'nightly');
    await page.waitForSelector('#confirm[open]', { timeout: 3000 });
    const copy = await page.textContent('#confirm-copy');
    await page.click('#confirm-action');
    await sleep(300);
    const [request] = (await calls(page, 'operate')).map((a) => a.request);
    const bound = request && request.channel === 'stable' && request.version === '2.0.0';
    assert(bound, `dialog "${copy.slice(0, 60)}" submitted ${JSON.stringify(request)}`);
  },
  async 'N38 Restore confirms the file that was reviewed last'(page) {
    await page.evaluate(() => {
      const m = window.__mock;
      m.on('plan_restore', ({ path }) => m.later(path.endsWith('a.zip') ? 600 : 50, { sha256: `hash-${path}`, apps: [{ backup: { id: path.endsWith('a.zip') ? 'arcade.box' : 'arcade.find', version: '1', channel: 'stable', settings: ['x'], notIncluded: null }, installed: true }] }));
      m.on('restore_backup', () => []);
    });
    await tab(page, 'settings');
    await page.fill('#restore-path', '/tmp/a.zip');
    await page.click('text=Review restore');
    await sleep(50);
    await page.fill('#restore-path', '/tmp/b.zip');
    await page.click('text=Review restore');
    await sleep(900);
    await page.click('button.primary-action:has-text("Restore")');
    await page.waitForSelector('#confirm[open]');
    await page.click('#confirm-action');
    await sleep(200);
    const [args] = await calls(page, 'restore_backup');
    assert(args && args.sha256 === `hash-${args.path}`, `restore ${JSON.stringify(args)}`);
  },
  async 'N48 Notification checkbox follows restored preferences'(page) {
    await tab(page, 'settings');
    const box = page.getByLabel('Show notifications');
    await box.uncheck();
    await sleep(100);
    await page.evaluate(() => { window.__mock.state.preferences = { ...window.__mock.state.preferences, notificationsEnabled: true }; window.__mock.emit('registry-changed', null); });
    await sleep(400);
    assert(await box.isChecked(), 'checkbox stayed unchecked after preferences changed to enabled');
  },
  async 'N49 App details history belongs to the app shown'(page) {
    await page.evaluate(() => window.__mock.on('release_history', ({ id }) => window.__mock.later(id === 'arcade.box' ? 600 : 30, [{ version: `${id}-9.0`, publishedAt: 'x', prerelease: false, html: '' }])));
    await tab(page, 'apps');
    await page.locator('article#arcade\\.box').getByRole('button', { name: 'Details' }).click();
    await page.click('text=Load version history');
    await page.click('text=← Back to apps');
    await page.locator('article#arcade\\.find').getByRole('button', { name: 'Details' }).click();
    await page.click('text=Load version history');
    await sleep(900);
    const section = await page.textContent('section[aria-label="Release history"]');
    assert(section.includes('arcade.find-9.0') && !section.includes('arcade.box-9.0'), `history: ${section}`);
  },
  async 'Start all reports failures and opens apps in the background'(page) {
    await page.evaluate(() => window.__mock.on('launch_app', ({ id }) => { if (id === 'arcade.clipboard') throw { code: 'launch', message: 'It exited right away.' }; return null; }));
    await tab(page, 'home');
    await page.click('text=Open all stopped apps');
    await sleep(500);
    const launches = await calls(page, 'launch_app');
    const message = await page.textContent('.shell-main');
    assert(launches.length === 7, `${launches.length} launches`);
    assert(launches.every((l) => l.mode === 'background'), `modes ${launches.map((l) => l.mode)}`);
    assert(/Clipboard/.test(message) && /6 of 7|Started 6/.test(message), `message: ${message.replace(/\s+/g, ' ').slice(0, 200)}`);
  },
  async 'Confirmation keeps line breaks'(page) {
    await withUpdate(page, 'arcade.look', '2.0.0', 'n');
    await page.evaluate(() => { const m = window.__mock; m.state.updates.apps['arcade.box'] = m.state.updates.apps['arcade.look']; m.emit('updates-changed', m.state.updates); });
    await tab(page, 'updates');
    await page.click('text=Review all updates');
    await page.waitForSelector('#confirm[open]');
    const ws = await page.$eval('#confirm-copy', (el) => getComputedStyle(el).whiteSpace);
    assert(ws === 'pre-line' || ws === 'pre-wrap', `white-space ${ws}`);
  },
};
