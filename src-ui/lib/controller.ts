import { tick } from 'svelte';
import { api, type AppView, type Channel, type ManagerFailure, type Operation, type OperationProgress, type OperationRequest } from './api';
import { askForConfirmation, forgetRelease, operationRequest, rememberRelease, ui } from './state.svelte';

let pendingInstall: string | null = null;
let reviewingInstall = false;

export function receiveInstallRequest(id: string): void {
  pendingInstall = id;
  void attempt(reviewHandoff);
}

export function setMessage(message: string, error = false): void {
  ui.message = message;
  ui.messageError = error;
}

export function asFailure(error: unknown): ManagerFailure {
  if (typeof error === 'object' && error !== null && 'message' in error) return error as ManagerFailure;
  return { code: 'error', message: String(error) };
}

export async function attempt(action: () => Promise<void>, appName?: string): Promise<void> {
  try {
    await action();
  } catch (error) {
    setMessage(`${appName ? `${appName}: ` : ''}${asFailure(error).message}`, true);
  }
}

export async function refresh(): Promise<void> {
  if (ui.refreshing) {
    ui.refreshPending = true;
    return;
  }
  ui.refreshing = true;
  try {
    ui.apps = await api.listApps();
    ui.loaded = true;
    ui.updates = await api.updateState();
    ui.toolsApp = await api.toolsApp();
    const preferences = await api.preferences();
    ui.autoUpdate = preferences.updates.autoUpdate;
    ui.updateChecksEnabled = preferences.updates.enabled;
    ui.notificationsEnabled = preferences.notificationsEnabled;
    ui.linkEnabled = preferences.linkEnabled;
    ui.theme = preferences.theme;
    document.documentElement.dataset.theme = ui.theme;
  } finally {
    ui.refreshing = false;
    if (ui.refreshPending) {
      ui.refreshPending = false;
      await refresh();
    }
  }
}

export async function initialize(): Promise<void> {
  const bootstrap = await api.initialize();
  ui.apps = bootstrap.apps;
  ui.loaded = true;
  ui.updates = bootstrap.updates;
  ui.selected = bootstrap.selected;
  ui.testing = bootstrap.testing;
  ui.linkEnabled = bootstrap.preferences.linkEnabled;
  ui.updateChecksEnabled = bootstrap.preferences.updates.enabled;
  ui.autoUpdate = bootstrap.preferences.updates.autoUpdate;
  ui.toolsApp = await api.toolsApp();
  ui.notificationsEnabled = bootstrap.preferences.notificationsEnabled;
  ui.theme = bootstrap.preferences.theme;
  document.documentElement.dataset.theme = ui.theme;
  ui.diagnostics = bootstrap.diagnostics.map(([name, value]) => `${name}: ${value}`).join('\n');
  setMessage(bootstrap.selected ? 'Review the selected app before installing.' : 'Choose an app to install, or check for updates.');
  if (bootstrap.selected) {
    pendingInstall = bootstrap.selected;
    await reviewHandoff();
  } else if (!bootstrap.testing && !ui.tabChosen) ui.tab = bootstrap.preferences.onboardingComplete ? 'home' : 'onboarding';
}

export async function changeChannel(app: AppView, channel: Channel): Promise<void> {
  await api.setChannel(app.id, channel);
  forgetRelease(app.id);
  await refresh();
  setMessage(`Selected ${channel} for ${app.name}. Check for updates to switch versions.`);
}

export async function changeLogin(app: AppView, enabled: boolean): Promise<void> {
  try {
    await api.setLogin(app.id, enabled);
  } finally {
    await refresh();
  }
}

export async function toggleLink(enabled: boolean): Promise<void> {
  await api.setLink(enabled);
  ui.linkEnabled = enabled;
  setMessage(enabled ? 'Arcade Link is enabled.' : 'Arcade Link is disabled. App management remains available.');
}

export async function launchApp(app: AppView, mode: 'foreground' | 'background'): Promise<void> {
  await api.launchApp(app.id, mode);
  await refresh();
}

export async function openReleases(app: AppView): Promise<void> {
  await api.openReleases(app.id);
}

export async function checkUpdates(app: AppView): Promise<void> {
  setMessage(`Checking ${app.name}…`);
  const release = await api.checkRelease(app.id);
  rememberRelease(release);
  setMessage(release.updateAvailable
    ? `${app.name} ${release.version} is available on ${release.channel}.`
    : `${app.name} is up to date.`);
}

export async function reviewUpdate(app: AppView): Promise<void> {
  setMessage(`Checking ${app.name}…`);
  const release = await api.checkRelease(app.id);
  rememberRelease(release);
  if (!release.updateAvailable) {
    setMessage(`${app.name} is up to date.`);
    return;
  }
  await confirmOperation(app, 'update');
}

export async function setUpdateChecks(enabled: boolean): Promise<void> {
  const previous = ui.updateChecksEnabled;
  ui.updateChecksEnabled = enabled;
  try {
    await api.setUpdateChecks(enabled);
    setMessage(enabled ? 'Scheduled update checks are enabled.' : 'Scheduled update checks are paused.');
  } catch (error) {
    ui.updateChecksEnabled = previous;
    throw error;
  }
}

export async function setAutoUpdate(app: AppView, enabled: boolean): Promise<void> {
  const previous = ui.autoUpdate[app.id] ?? false;
  ui.autoUpdate = { ...ui.autoUpdate, [app.id]: enabled };
  try {
    await api.setAutoUpdate(app.id, enabled);
  } catch (error) {
    ui.autoUpdate = { ...ui.autoUpdate, [app.id]: previous };
    throw error;
  }
}

export async function checkAllUpdates(): Promise<void> {
  if (ui.updateChecking) return;
  ui.updateChecking = true;
  try {
    ui.updates = await api.checkUpdates();
    setMessage(ui.updates.lastFailure?.reason ?? 'Update check complete.');
  } finally {
    ui.updateChecking = false;
  }
}

export async function checkUpdatesOnWindowOpen(): Promise<void> {
  if (ui.updateChecking) return;
  ui.updateChecking = true;
  try {
    ui.updates = await api.checkUpdatesWindowOpen();
  } finally {
    ui.updateChecking = false;
  }
}

/** Start every stopped app in the background, like the tray's Start all. */
export async function launchAll(): Promise<void> {
  const stopped = ui.apps.filter((app) => app.installed && !app.running);
  const failed: string[] = [];
  for (const app of stopped) {
    try { await api.launchApp(app.id, 'background'); }
    catch (error) { failed.push(`${app.name}: ${asFailure(error).message}`); }
  }
  await refresh();
  const started = stopped.length - failed.length;
  if (!stopped.length) setMessage('All installed apps are already running.');
  else if (failed.length) setMessage(`Started ${started} of ${stopped.length} apps.\n${failed.join('\n')}`, true);
  else setMessage(`Started ${started} app${started === 1 ? '' : 's'} in the background.`);
}

export async function reviewInstall(app: AppView): Promise<void> {
  setMessage(`Checking ${app.name}…`);
  rememberRelease(await api.checkRelease(app.id));
  await confirmOperation(app, 'install');
}

export async function reviewHandoff(): Promise<void> {
  if (reviewingInstall || ui.busy || ui.dialog || !pendingInstall) return;
  reviewingInstall = true;
  try {
    while (pendingInstall && !ui.busy && !ui.dialog) {
      const id = pendingInstall;
      pendingInstall = null;
      ui.tab = 'apps';
      ui.selected = id;
      await tick();
      document.getElementById(id)?.scrollIntoView({
        block: 'center',
        behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
      });
      const app = ui.apps.find((candidate) => candidate.id === id);
      if (!app) continue;
      if (app.installed || app.managed) {
        setMessage(`${app.name} is already installed. Review its update or repair options.`);
      } else {
        await reviewInstall(app);
      }
    }
  } finally {
    reviewingInstall = false;
  }
}

export async function confirmOperation(app: AppView, operation: Operation): Promise<void> {
  const labels: Record<Operation, string> = {
    install: 'Install', update: 'Update', repair: 'Repair', uninstall: 'Remove',
    adopt: 'Adopt', rollback: 'Roll back',
  };
  const release = ui.releases.get(app.id);
  const title = `${labels[operation]} ${app.name}${release && ['install', 'update', 'repair'].includes(operation) ? ` ${release.version}` : ''}?`;
  // The channel the release was looked up on, which is what the backend
  // must install: the user may change the app's channel while this is open.
  const channel = release?.channel ?? app.channel;
  const copy = operation === 'adopt'
    ? 'Let Tools manage this installation by running the app’s own installer. Settings and data are kept. Development builds are left unchanged.'
    : operation === 'rollback'
    ? `Reinstall the saved version (${app.rollbackVersion ?? 'saved build'}) and reopen the app if it was running. Settings and data are kept.`
    : operation === 'uninstall'
    ? 'The app will close and its installation will be removed. Settings and data are kept unless you select the option below.'
    : operation === 'install'
      ? `Install the ${channel} release for your user account and open the app.`
      : `Download a fresh ${channel} release, close the app, and replace its installation. Settings and data are kept.${app.reason ? `\n${app.reason}` : ''}`;
  const choice = await askForConfirmation({
    title,
    copy,
    action: labels[operation],
    destructive: operation === 'uninstall',
    kind: operation === 'uninstall' ? 'remove-data' : operation === 'install' ? 'background' : null,
    folders: operation === 'uninstall' ? app.dataFolders : [],
  });
  if (!choice.confirmed) return;
  if (app.id === 'arcade.tools' && operation === 'rollback') {
    await withBusy(api.selfRollback, 'Rolling back Arcade Tools');
    return;
  }
  const request = operationRequest(app.id, operation);
  if (['install', 'update', 'repair'].includes(operation)) {
    request.channel = channel;
    request.version = release?.version ?? null;
  }
  request.mode = operation === 'install' ? (choice.background ? 'background' : 'foreground') : null;
  request.removeData = choice.removeData;
  await runOperation(request);
}

export async function cancelOperation(): Promise<void> {
  await api.cancelOperation();
  setMessage('Cancelling…');
}

export function appName(id: string): string {
  if (id === 'arcade.tools') return 'Arcade Tools';
  return ui.apps.find((app) => app.id === id)?.name ?? id;
}

export function receiveProgress(progress: OperationProgress): void {
  const name = appName(progress.id);
  ui.phase = `${name} · ${progress.phase}`;
  ui.progress = progress.fraction;
  ui.cancellable = progress.cancellable;
}

export async function runOperation(request: OperationRequest): Promise<void> {
  ui.busy = true;
  try {
    const text = await api.operate(request);
    forgetRelease(request.id);
    setMessage(text);
  } catch (error) {
    const problem = asFailure(error);
    const name = appName(request.id);
    ui.busy = false;
    if (problem.code === 'cancelled') {
      setMessage(`${name}: cancelled. Nothing was changed.`);
    } else {
      setMessage(`${name}: ${problem.message}`, true);
    }
    if (problem.code === 'mode_required') {
      const choice = await askForConfirmation({
        title: 'Choose how to reopen the app',
        copy: problem.message,
        action: 'Continue',
        destructive: false,
        kind: 'reopen-mode',
        folders: [],
      });
      if (choice.confirmed) {
        request.mode = choice.mode;
        return await runOperation(request);
      }
    } else if (problem.code === 'disconnected') {
      const choice = await askForConfirmation({
        title: 'Close the app first',
        copy: problem.message,
        action: 'Retry',
        destructive: false,
        kind: 'app-closed',
        folders: [],
      });
      if (choice.confirmed && choice.appClosed) {
        request.appClosed = true;
        return await runOperation(request);
      }
    }
  } finally {
    ui.busy = false;
    await refresh();
    void attempt(reviewHandoff);
  }
}
/** Tools' own update, confirmed like every app update. */
export async function confirmSelfUpdate(): Promise<void> {
  const tools = ui.toolsApp;
  const latest = tools ? ui.updates?.apps[tools.id]?.latest[tools.channel]?.latestVersion : null;
  const choice = await askForConfirmation({
    title: `Update Arcade Tools${latest ? ` to ${latest}` : ''}?`,
    copy: 'Tools downloads the latest release, closes, and reopens itself. Your apps keep running.' + (tools?.reason ? `\n${tools.reason}` : ''),
    action: 'Update', destructive: false, kind: null, folders: [],
  });
  if (choice.confirmed) await withBusy(api.selfUpdate, 'Updating Arcade Tools');
}
export async function withBusy(action: () => Promise<void>, phase: string): Promise<void> {
  ui.busy = true;
  ui.phase = phase;
  ui.progress = null;
  try { await action(); }
  finally { ui.busy = false; await refresh(); }
}
export async function installBundle(bundleId: string, ids: string[]): Promise<void> {
  await withBusy(async () => { const messages = await api.installCatalogBundle(bundleId, ids); setMessage(messages.join(' ')); }, 'Installing bundle');
}
export function openLearn(id: string): Promise<void> {
  ui.learnTarget = id;
  ui.tab = 'learn';
  return Promise.resolve();
}
export async function updateAll(apps: AppView[]): Promise<void> {
  const available = apps.filter((app) => ui.updates?.apps[app.id]?.updateAvailable && !ui.updates.apps[app.id]?.heldBack && app.managed);
  if (!available.length) return;
  const choice = await askForConfirmation({ title: `Update ${available.length} apps?`, copy: available.map((app) => `• ${app.name} → ${ui.updates?.apps[app.id]?.latest[app.channel]?.latestVersion ?? 'latest'}${app.reason ? `\n  ${app.reason}` : ''}`).join('\n') + '\nBusy apps will be left running. Settings and data are kept.', action: 'Update all', destructive: false, kind: null, folders: [] });
  if (!choice.confirmed) return;
  for (const app of available) {
    if (app.id === 'arcade.tools') continue;
    await runOperation(operationRequest(app.id, 'update'));
    if (ui.messageError) return;
  }
  if (available.some((app) => app.id === 'arcade.tools')) await withBusy(api.selfUpdate, 'Updating Arcade Tools');
}
