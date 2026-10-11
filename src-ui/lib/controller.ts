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
    ui.updates = await api.updateState();
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
  ui.updates = bootstrap.updates;
  ui.selected = bootstrap.selected;
  ui.testing = bootstrap.testing;
  ui.linkEnabled = bootstrap.preferences.linkEnabled;
  ui.updateChecksEnabled = bootstrap.preferences.updates.enabled;
  ui.autoUpdate = bootstrap.preferences.updates.autoUpdate;
  ui.diagnostics = bootstrap.diagnostics.map(([name, value]) => `${name}: ${value}`).join('\n');
  setMessage(bootstrap.selected ? 'Review the selected app before installing.' : 'Choose an app to install, or check for updates.');
  if (bootstrap.selected) {
    pendingInstall = bootstrap.selected;
    await reviewHandoff();
  } else if (!bootstrap.testing && !ui.apps.some((app) => app.installed)) {
    let complete = false;
    try { complete = localStorage.getItem('arcade-tools-onboarding-complete') === 'yes'; } catch { /* continue this session */ }
    if (!complete) ui.tab = 'onboarding';
  }
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

export async function launchAll(): Promise<void> {
  const installed = ui.apps.filter((app) => app.installed && !app.running);
  for (const app of installed) await api.launchApp(app.id, 'foreground');
  await refresh();
  setMessage(installed.length ? `Opened ${installed.length} app${installed.length === 1 ? '' : 's'}.` : 'All installed apps are already running.');
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
  };
  const release = ui.releases.get(app.id);
  const title = `${labels[operation]} ${app.name}${release && operation !== 'uninstall' ? ` ${release.version}` : ''}?`;
  const copy = operation === 'uninstall'
    ? 'The app will close and its installation will be removed. Settings and data are kept unless you select the option below.'
    : operation === 'install'
      ? `Install the ${app.channel} release for your user account and open the app.`
      : `Download a fresh ${app.channel} release, close the app, and replace its installation. Settings and data are kept.`;
  const choice = await askForConfirmation({
    title,
    copy,
    action: labels[operation],
    destructive: operation === 'uninstall',
    kind: operation === 'uninstall' ? 'remove-data' : operation === 'install' ? 'background' : null,
    folders: operation === 'uninstall' ? app.dataFolders : [],
  });
  if (!choice.confirmed) return;
  const request = operationRequest(app.id, operation);
  request.mode = operation === 'install' ? (choice.background ? 'background' : 'foreground') : null;
  request.removeData = choice.removeData;
  await runOperation(request);
}

export async function cancelOperation(): Promise<void> {
  await api.cancelOperation();
  setMessage('Cancelling…');
}

export function receiveProgress(progress: OperationProgress): void {
  const appName = ui.apps.find((app) => app.id === progress.id)?.name ?? progress.id;
  ui.phase = `${appName} · ${progress.phase}`;
  ui.progress = progress.fraction;
  ui.cancellable = progress.cancellable;
}

async function runOperation(request: OperationRequest): Promise<void> {
  ui.busy = true;
  try {
    const text = await api.operate(request);
    forgetRelease(request.id);
    setMessage(text);
  } catch (error) {
    const problem = asFailure(error);
    const name = ui.apps.find((app) => app.id === request.id)?.name ?? request.id;
    setMessage(`${name}: ${problem.message}`, true);
    ui.busy = false;
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
