<script lang="ts">
  import { onMount, tick } from 'svelte';
  import AppDialog from './components/ConfirmDialog.svelte';
  import Message from './components/Message.svelte';
  import ProgressBar from './components/ProgressBar.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import { api, appEvents, type AppView, type Channel, type Operation, type UiControlReport, type UiRenderReport } from './lib/api';
  import {
    attempt, cancelOperation, changeChannel, changeLogin, checkAllUpdates, checkUpdates, checkUpdatesOnWindowOpen, confirmOperation,
    initialize, launchAll, launchApp, openReleases, receiveInstallRequest, receiveProgress, refresh, reviewInstall, reviewUpdate, setAutoUpdate, setMessage, toggleLink,
  } from './lib/controller';
  import Apps from './tabs/Apps.svelte';
  import Connections from './tabs/Connections.svelte';
  import Engines from './tabs/Engines.svelte';
  import Home from './tabs/Home.svelte';
  import Learn from './tabs/Learn.svelte';
  import Onboarding from './tabs/Onboarding.svelte';
  import Settings from './tabs/Settings.svelte';
  import Updates from './tabs/Updates.svelte';
  import { ui, type Theme } from './lib/state.svelte';
  import { tabs, type ShellTab } from './lib/tabs';

  let revision = 0;
  let linkInput: HTMLInputElement;
  let updateTimer: number | undefined;
  let initialized = false;
  let hasSeenFocus = false;
  const activeTabLabel = $derived(tabs.find((tab) => tab.id === ui.tab)?.label ?? 'Apps');

  function run(action: () => Promise<void>, appName?: string): void {
    void attempt(action, appName);
  }

  function updateTheme(theme: Theme): void {
    ui.theme = theme;
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem('arcade-tools-theme', theme);
    } catch {
      // The theme remains active for this session if storage is unavailable.
    }
  }

  function selectTab(tab: ShellTab): void {
    ui.tab = tab;
  }

  function sendRenderedReport(snapshot: Omit<UiRenderReport, 'revision' | 'controls' | 'tab' | 'visiblePanel'>): void {
    const controls = [...document.querySelectorAll<HTMLElement>('button, select, input')].map((control): UiControlReport => {
      const rect = control.getBoundingClientRect();
      const item: UiControlReport = {
        label: control.getAttribute('aria-label') ?? control.textContent ?? '',
        id: control.id,
        disabled: 'disabled' in control && Boolean(control.disabled),
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
      };
      const appId = control.closest('article')?.id;
      if (appId) item.app = appId;
      if (control instanceof HTMLInputElement) item.checked = control.checked;
      return item;
    });
    const dialog = document.querySelector<HTMLDialogElement>('#confirm');
    const report: UiRenderReport = {
      revision: ++revision,
      tab: ui.tab,
      visiblePanel: document.querySelector<HTMLElement>('.tab-panel:not([hidden])')?.id ?? null,
      ...snapshot,
      dialog: dialog?.open ? document.querySelector('#confirm-title')?.textContent ?? null : null,
      controls,
    };
    void api.uiRendered(report).catch(() => undefined);
  }

  $effect(() => {
    const enabled = ui.testing;
    // These shell states also alter control visibility, checked values or positions.
    void ui.tab;
    void ui.linkEnabled;
    void ui.cancellable;
    void ui.progress;
    void ui.theme;
    void ui.messageError;
    void ui.dialog?.removeData;
    void ui.dialog?.background;
    void ui.dialog?.mode;
    void ui.dialog?.appClosed;
    const snapshot = {
      apps: ui.apps,
      selected: ui.selected,
      busy: ui.busy,
      message: ui.message,
      dialog: ui.dialog?.title ?? null,
      phase: ui.phase,
    };
    if (!enabled) return;
    void tick().then(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))).then(() => {
      sendRenderedReport(snapshot);
    });
  });

  onMount(() => {
    let unlisten: (() => void)[] = [];
    let disposed = false;
    const onScroll = () => {
      if (ui.testing) {
        const snapshot = {
          apps: ui.apps, selected: ui.selected, busy: ui.busy, message: ui.message,
          dialog: document.querySelector<HTMLDialogElement>('#confirm')?.open
            ? document.querySelector('#confirm-title')?.textContent ?? null : null,
          phase: ui.phase,
        };
        void tick().then(() => sendRenderedReport(snapshot));
      }
    };
    const unlistenResize = () => window.removeEventListener('resize', onScroll);
    const onFocus = () => {
      if (!initialized) return;
      if (!hasSeenFocus) { hasSeenFocus = true; return; }
      if (ui.testing || !ui.updateChecksEnabled) return;
      const stale = !ui.updates?.lastCheck || Date.now() / 1000 - ui.updates.lastCheck > 60 * 60;
      if (stale) void attempt(checkUpdatesOnWindowOpen);
    };

    try {
      const storedTheme = localStorage.getItem('arcade-tools-theme');
      if (storedTheme === 'light' || storedTheme === 'dark' || storedTheme === 'system') ui.theme = storedTheme;
    } catch {
      // Use the system theme when local storage is unavailable.
    }
    document.documentElement.dataset.theme = ui.theme;

    void (async () => {
      try {
        const handlers = await Promise.all([
          appEvents.operationProgress((progress) => receiveProgress(progress)),
          appEvents.installRequest((id) => receiveInstallRequest(id)),
          appEvents.registryChanged(() => { void attempt(refresh); }),
          appEvents.managerMessage((message) => setMessage(message, true)),
        ]);
        if (disposed) handlers.forEach((stop) => stop());
        else unlisten = handlers;
        await initialize();
        initialized = true;
        hasSeenFocus = true;
        if (!ui.testing) {
          const scheduleUpdateCheck = (delay: number) => {
            if (updateTimer !== undefined) window.clearTimeout(updateTimer);
            updateTimer = window.setTimeout(async () => {
              if (ui.updateChecksEnabled && navigator.onLine) await attempt(checkAllUpdates);
              scheduleUpdateCheck(6 * 60 * 60 * 1000);
            }, delay);
          };
          scheduleUpdateCheck(60 * 1000);
        }
      } catch (error) {
        setMessage(error instanceof Error ? error.message : String(error), true);
      }
    })();

    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll);
    window.addEventListener('focus', onFocus);
    return () => {
      disposed = true;
      unlisten.forEach((stop) => stop());
      unlistenResize();
      window.removeEventListener('focus', onFocus);
      if (updateTimer !== undefined) window.clearTimeout(updateTimer);
      window.removeEventListener('scroll', onScroll);
    };
  });

  async function setLink(enabled: boolean): Promise<void> {
    const previous = ui.linkEnabled;
    ui.linkEnabled = enabled;
    try {
      await toggleLink(enabled);
    } catch (error) {
      ui.linkEnabled = previous;
      if (linkInput) linkInput.checked = previous;
      throw error;
    }
  }

  function install(app: AppView): Promise<void> {
    return reviewInstall(app);
  }

  function operation(app: AppView, action: Operation): Promise<void> {
    return confirmOperation(app, action);
  }

  function channel(app: AppView, value: Channel): Promise<void> {
    return changeChannel(app, value);
  }

  function login(app: AppView, enabled: boolean): Promise<void> {
    return changeLogin(app, enabled);
  }
</script>

<svelte:head>
  <title>Arcade Tools</title>
  <meta name="description" content="Install, update and take care of your Arcade apps." />
</svelte:head>

<div class="app-shell">
  <Sidebar active={ui.tab} onSelect={selectTab} />
  <div class="shell-main">
    <header class="utility-header">
      <div class="header-context"><span class="context-dot"></span><span>Arcade Tools</span><span class="context-divider">/</span><span>{activeTabLabel}</span></div>
      <div class="header-actions">
        <label class="link-toggle"><input id="link" bind:this={linkInput} type="checkbox" checked={ui.linkEnabled} disabled={ui.busy} onchange={(event) => run(() => setLink(event.currentTarget.checked))} /> <span>Connect with other Arcade apps</span></label>
        <button id="refresh" class="refresh-button" type="button" disabled={ui.busy} onclick={() => run(refresh)}>Refresh</button>
      </div>
    </header>
    <Message text={ui.message} error={ui.messageError} />
    <main class="page-main">
      <Home
        active={ui.tab === 'home'}
        apps={ui.apps}
        updates={ui.updates}
        busy={ui.busy}
        onOpen={(app) => launchApp(app, 'foreground')}
        onStartAll={launchAll}
        onCheckNow={checkAllUpdates}
        onSelectTab={selectTab}
        {run}
      />
      <Apps
        active={ui.tab === 'apps'}
        apps={ui.apps}
        selected={ui.selected}
        releases={ui.releases}
        busy={ui.busy}
        onChannel={channel}
        onLogin={login}
        onLaunch={launchApp}
        onInstall={install}
        onCheckUpdates={checkUpdates}
        onOperation={operation}
        onReleases={openReleases}
        {run}
      />
      <Updates
        active={ui.tab === 'updates'}
        apps={ui.apps}
        updates={ui.updates}
        autoUpdate={ui.autoUpdate}
        busy={ui.busy}
        checking={ui.updateChecking}
        updateChecksEnabled={ui.updateChecksEnabled}
        onCheckNow={checkAllUpdates}
        onUpdate={reviewUpdate}
        onAutoUpdate={setAutoUpdate}
        onSelectApps={() => selectTab('apps')}
        {run}
      />
      <Learn active={ui.tab === 'learn'} {run} />
      <Connections active={ui.tab === 'connections'} onSettings={() => selectTab('settings')} {run} />
      <Engines active={ui.tab === 'engines'} {run} />
      <Settings
        active={ui.tab === 'settings'}
        theme={ui.theme}
        linkEnabled={ui.linkEnabled}
        diagnostics={ui.diagnostics}
        busy={ui.busy}
        onTheme={updateTheme}
        onLink={setLink}
        {run}
      />
      <Onboarding
        active={ui.tab === 'onboarding'}
        apps={ui.apps}
        onInstall={install}
        onComplete={() => selectTab('home')}
        {run}
      />
    </main>
    <footer class="shell-footer"><span>Arcade Tools · 0.1.0</span><span>Apps stay independent; Tools makes them easier to manage.</span></footer>
  </div>
</div>
<ProgressBar visible={ui.busy} phase={ui.phase} progress={ui.progress} cancellable={ui.cancellable} onCancel={() => run(cancelOperation)} />
<AppDialog />
