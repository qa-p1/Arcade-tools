import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './style.css';

type Channel = 'stable' | 'nightly';
type Mode = 'foreground' | 'background';
type Operation = 'install' | 'update' | 'repair' | 'uninstall';
interface App {
  id: string; name: string; pitch: string; version: string | null; managed: boolean; installed: boolean;
  running: boolean; channel: Channel; healthy: boolean; startAtLogin: boolean; loginSupported: boolean;
  dataFolders: string[]; reason: string | null;
}
interface Release { id: string; version: string; channel: Channel; file: string; updateAvailable: boolean }
interface Progress { id: string; phase: string; fraction: number | null; cancellable: boolean }
interface Request { id: string; operation: Operation; mode: Mode | null; removeData: boolean; appClosed: boolean }
interface Bootstrap { apps: App[]; preferences: { linkEnabled: boolean }; selected: string | null; diagnostics: [string, string][]; testing: boolean }
interface Failure { code: string; message: string }

function element<T extends HTMLElement>(selector: string): T {
  const node = document.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node;
}
function node<K extends keyof HTMLElementTagNameMap>(tag: K, text = '', className = ''): HTMLElementTagNameMap[K] {
  const n = document.createElement(tag); n.textContent = text; n.className = className; return n;
}
const appsNode = element('#apps');
const message = element('#message');
const dialog = element<HTMLDialogElement>('#confirm');
const releases = new Map<string, Release>();
let apps: App[] = [];
let selected: string | null = null;
let busy = false;
let refreshing = false;
let refreshPending = false;
let testing = false;
let pendingInstall: string | null = null;
let reviewingInstall = false;
let revision = 0;

function report() {
  if (!testing) return;
  const controls = [...document.querySelectorAll<HTMLElement>('button, select, input')].map(control => {
    const rect = control.getBoundingClientRect();
    return { label: control.getAttribute('aria-label') ?? control.textContent ?? '', id: control.id,
      app: control.closest('article')?.id, checked: control instanceof HTMLInputElement ? control.checked : undefined,
      disabled: 'disabled' in control && control.disabled, x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  });
  void invoke('ui_rendered', { report: { revision: ++revision, apps, selected, busy, message: message.textContent,
    dialog: dialog.open ? element('#confirm-title').textContent : null, controls,
    phase: element('#operation-text').textContent } });
}

function failure(error: unknown): Failure {
  if (typeof error === 'object' && error !== null && 'message' in error) return error as Failure;
  return { code: 'error', message: String(error) };
}
function say(text: string, error = false) { message.textContent = text; message.classList.toggle('error', error); report(); }
async function attempt(action: () => Promise<void>, appName?: string) { try { await action(); } catch (error) { say(`${appName ? `${appName}: ` : ''}${failure(error).message}`, true); } }
function button(text: string, action: () => Promise<void>, className = '') {
  const n = node('button', text, className); n.disabled = busy; n.addEventListener('click', () => { void attempt(action, apps.find(a => a.id === n.closest('article')?.id)?.name); }); return n;
}
function selectApp(id: string) { selected = id; render(); document.getElementById(id)?.scrollIntoView({ block: 'center', behavior: 'smooth' }); }

function render() {
  appsNode.replaceChildren(...apps.map(app => {
    const card = node('article', '', selected === app.id ? 'selected' : ''); card.id = app.id;
    const glyph = node('img'); glyph.src = `/glyphs/${app.id}.svg`; glyph.alt = ''; glyph.width = 30; glyph.height = 30;
    const info = node('div', '', 'info');
    const heading = node('div', '', 'card-heading'); heading.append(node('h2', app.name));
    heading.append(node('span', app.running ? 'Running' : app.installed ? 'Installed' : app.managed ? 'Needs repair' : 'Not installed', 'badge'));
    info.append(heading, node('p', app.pitch, 'pitch'));
    const controls = node('div', '', 'controls');
    const channelLabel = node('label', 'Channel '); const channel = node('select'); channel.setAttribute('aria-label', `${app.name} channel`);
    for (const [value, title] of [['stable', 'Stable'], ['nightly', 'Nightly']]) { const option = node('option', title); option.value = value; option.selected = app.channel === value; channel.append(option); }
    channel.disabled = busy || app.installed && !app.managed;
    channel.addEventListener('change', () => { void attempt(async () => { await invoke('set_channel', { id: app.id, channel: channel.value }); releases.delete(app.id); await refresh(); say(`Selected ${channel.value} for ${app.name}. Check for updates to switch versions.`); }); });
    channelLabel.append(channel); controls.append(channelLabel);
    if (app.version) controls.append(node('span', `v${app.version}`, 'version'));
    if (app.installed && app.loginSupported) {
      const loginLabel = node('label', '', 'login'); const input = node('input'); input.type = 'checkbox'; input.checked = app.startAtLogin; input.disabled = busy;
      input.setAttribute('aria-label', `${app.name} start at login`);
      input.addEventListener('change', () => { void attempt(async () => { try { await invoke('set_login', { id: app.id, enabled: input.checked }); } finally { await refresh(); } }); });
      loginLabel.append(input, document.createTextNode(' Start at login')); controls.append(loginLabel);
    }
    if (app.installed && !app.loginSupported) controls.append(node('span', 'Start at login: use the app’s settings', 'version'));
    info.append(controls);
    if (app.reason) info.append(node('p', app.reason, 'reason'));
    const actions = node('div', '', 'actions');
    if (app.installed) {
      actions.append(button('Open', async () => { await invoke('launch_app', { id: app.id, mode: 'foreground' }); await refresh(); }, 'primary'));
      actions.append(button('Background', async () => { await invoke('launch_app', { id: app.id, mode: 'background' }); await refresh(); }));
    }
    if (!app.installed && !app.managed) actions.append(button('Check & install', () => reviewInstall(app), 'primary'));
    if (app.managed) {
      actions.append(button('Check updates', async () => {
        say(`Checking ${app.name}…`); const release = await invoke<Release>('check_release', { id: app.id }); releases.set(app.id, release); render();
        say(release.updateAvailable ? `${app.name} ${release.version} is available on ${release.channel}.` : `${app.name} is up to date.`);
      }));
      if (releases.get(app.id)?.updateAvailable) actions.append(button('Update', () => confirmOperation(app, 'update'), 'primary'));
      actions.append(button('Repair', () => confirmOperation(app, 'repair')));
      actions.append(button('Remove', () => confirmOperation(app, 'uninstall'), 'quiet danger'));
    }
    actions.append(button('Releases ↗', async () => { await invoke('open_releases', { id: app.id }); }, 'quiet'));
    card.append(glyph, info, actions); return card;
  }));
  element<HTMLButtonElement>('#refresh').disabled = busy;
  element<HTMLInputElement>('#link').disabled = busy;
  report();
}
async function refresh() {
  if (refreshing) { refreshPending = true; return; }
  refreshing = true;
  try { apps = await invoke<App[]>('list_apps'); render(); } finally {
    refreshing = false;
    if (refreshPending) { refreshPending = false; await refresh(); }
  }
}
function confirm(title: string, copy: string, action: string, extra: HTMLElement[] = [], destructive = false): Promise<boolean> {
  element('#confirm-title').textContent = title; element('#confirm-copy').textContent = copy;
  element('#confirm-action').textContent = action; element('#confirm-action').className = destructive ? 'destructive' : 'primary'; element('#confirm-extra').replaceChildren(...extra);
  dialog.returnValue = ''; dialog.showModal();
  report();
  return new Promise(resolve => { dialog.addEventListener('close', () => { report(); resolve(dialog.returnValue === 'confirm'); }, { once: true }); });
}
async function reviewInstall(app: App) {
  say(`Checking ${app.name}…`);
  const release = await invoke<Release>('check_release', { id: app.id });
  releases.set(app.id, release);
  await confirmOperation(app, 'install');
}
async function reviewHandoff() {
  if (reviewingInstall || busy || dialog.open || !pendingInstall) return;
  reviewingInstall = true;
  try {
    while (pendingInstall && !busy && !dialog.open) {
      const id = pendingInstall; pendingInstall = null; selectApp(id);
      const app = apps.find(a => a.id === id);
      if (!app) continue;
      if (app.installed || app.managed) say(`${app.name} is already installed. Review its update or repair options.`);
      else await reviewInstall(app);
    }
  } finally { reviewingInstall = false; }
}
function checkbox(text: string) {
  const label = node('label', '', 'dialog-option'); const input = node('input'); input.type = 'checkbox'; input.setAttribute('aria-label', text); label.append(input, document.createTextNode(` ${text}`)); return { label, input };
}
async function confirmOperation(app: App, operation: Operation) {
  const remove = checkbox('Remove settings and data'); const background = checkbox('Open in background');
  const folders = node('pre', app.dataFolders.join('\n'), 'folders');
  const release = releases.get(app.id);
  const titles = { install: 'Install', update: 'Update', repair: 'Repair', uninstall: 'Remove' };
  const title = `${titles[operation]} ${app.name}${release && operation !== 'uninstall' ? ` ${release.version}` : ''}?`;
  const copy = operation === 'uninstall' ? 'The app will close and its installation will be removed. Settings and data are kept unless you select the option below.'
    : operation === 'install' ? `Install the ${app.channel} release for your user account and open the app.`
    : `Download a fresh ${app.channel} release, close the app, and replace its installation. Settings and data are kept.`;
  const extras = operation === 'uninstall' ? [remove.label, folders] : operation === 'install' ? [background.label] : [];
  if (!await confirm(title, copy, titles[operation], extras, operation === 'uninstall')) return;
  await runOperation({ id: app.id, operation, mode: operation === 'install' ? (background.input.checked ? 'background' : 'foreground') : null, removeData: remove.input.checked, appClosed: false });
}
async function runOperation(request: Request): Promise<void> {
  busy = true; render(); element('#operation').hidden = false;
  try {
    const text = await invoke<string>('operate', { request }); releases.delete(request.id); say(text);
  } catch (error) {
    const problem = failure(error); say(`${apps.find(a => a.id === request.id)?.name ?? request.id}: ${problem.message}`, true);
    busy = false; render(); element('#operation').hidden = true;
    if (problem.code === 'mode_required') {
      const label = node('label', 'Reopen the app '); const mode = node('select'); mode.setAttribute('aria-label', 'Reopen mode'); mode.append(node('option', 'In foreground'), node('option', 'In background'));
      mode.options[0].value = 'foreground'; mode.options[1].value = 'background'; label.append(mode);
      if (await confirm('Choose how to reopen the app', problem.message, 'Continue', [label])) { request.mode = mode.value as Mode; return await runOperation(request); }
    } else if (problem.code === 'disconnected') {
      const closed = checkbox('I have closed the app');
      if (await confirm('Close the app first', problem.message, 'Retry', [closed.label]) && closed.input.checked) { request.appClosed = true; return await runOperation(request); }
    }
  } finally { busy = false; element('#operation').hidden = true; await refresh(); void attempt(reviewHandoff); }
}

async function start() {
  await listen<Progress>('operation-progress', event => {
    const p = event.payload; element('#operation-text').textContent = `${apps.find(a => a.id === p.id)?.name ?? p.id} · ${p.phase}`;
    const progress = element<HTMLProgressElement>('#progress'); progress.max = 1; if (p.fraction === null) progress.removeAttribute('value'); else progress.value = p.fraction;
    element<HTMLButtonElement>('#cancel').disabled = !p.cancellable;
    report();
  });
  await listen<string>('install-request', event => { pendingInstall = event.payload; void attempt(reviewHandoff); });
  await listen('registry-changed', () => { void attempt(refresh); });
  await listen<string>('manager-message', event => say(event.payload, true));
  const data = await invoke<Bootstrap>('initialize'); apps = data.apps; selected = data.selected; testing = data.testing;
  element<HTMLInputElement>('#link').checked = data.preferences.linkEnabled;
  element('#diagnostics').textContent = data.diagnostics.map(([name, value]) => `${name}: ${value}`).join('\n');
  render(); say(selected ? 'Review the selected app before installing.' : 'Choose an app to install, or check for updates.');
  if (testing) {
    window.addEventListener('scroll', report, { passive: true });
    window.addEventListener('resize', report);
  }
  element('#refresh').addEventListener('click', () => { void attempt(refresh); });
  element('#cancel').addEventListener('click', () => { void attempt(async () => { await invoke('cancel_operation'); say('Cancelling…'); }); });
  element<HTMLInputElement>('#link').addEventListener('change', event => { void attempt(async () => { const input = event.target as HTMLInputElement;
    try { await invoke('set_link', { enabled: input.checked }); say(input.checked ? 'Arcade Link is enabled.' : 'Arcade Link is disabled. App management remains available.'); }
    catch (error) { input.checked = !input.checked; throw error; }
  }); });
  if (selected) { pendingInstall = selected; await reviewHandoff(); }
}
void attempt(start);
