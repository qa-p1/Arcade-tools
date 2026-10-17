<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import AppDialog from './components/ConfirmDialog.svelte';
  import Message from './components/Message.svelte';
  import ProgressBar from './components/ProgressBar.svelte';
  import Sidebar from './components/Sidebar.svelte';
  import { api, appEvents, type AppView, type Channel, type Operation, type UiControlReport, type UiRenderReport } from './lib/api';
  import {
    attempt, cancelOperation, changeChannel, changeLogin, checkAllUpdates, checkUpdates, checkUpdatesOnWindowOpen, confirmOperation,
    initialize, installBundle, openLearn, updateAll, withBusy, launchAll, launchApp, openReleases, receiveInstallRequest, receiveProgress, refresh, reviewInstall, reviewUpdate, setAutoUpdate, setMessage, toggleLink,
    setUpdateChecks,
  } from './lib/controller';
  import Apps from './tabs/Apps.svelte';
  import Engines from './tabs/Engines.svelte';
  import Home from './tabs/Home.svelte';
  import Onboarding from './tabs/Onboarding.svelte';
  import Settings from './tabs/Settings.svelte';
  import Updates from './tabs/Updates.svelte';
  import Shortcuts from './tabs/Shortcuts.svelte';
  import { ui, type Theme } from './lib/state.svelte';
  import { setShortcutsEnabled, tabs, type ShellTab } from './lib/tabs.svelte';

  let revision = 0;
  let linkInput: HTMLInputElement;
  let LearnView = $state<typeof import('./tabs/Learn.svelte').default | null>(null);
  let ConnectionsView = $state<typeof import('./tabs/Connections.svelte').default | null>(null);
  $effect(() => {
    if (ui.tab === 'learn' && !LearnView) void import('./tabs/Learn.svelte').then((module) => LearnView = module.default);
    if (ui.tab === 'connections' && !ConnectionsView) void import('./tabs/Connections.svelte').then((module) => ConnectionsView = module.default);
  });
  let installProposal = $state<{ title: string; copy: string; installed: boolean; primary: string; secondary: string; options: boolean } | null>(null);
  let installLogin = $state(true);
  let removeDownload = $state(true);
  let installDialog: HTMLDialogElement;
  $effect(() => { if (installProposal && installDialog && !installDialog.open) installDialog.showModal(); });
  async function confirmSelfInstall(): Promise<void> { await invoke('confirm_self_install', { startAtLogin: installLogin, removeDownload }); }
  let initialized = false;
  let hasSeenFocus = false;
  const activeTabLabel = $derived(tabs.find((tab) => tab.id === ui.tab)?.label ?? 'Apps');

  function run(action: () => Promise<void>, appName?: string): void {
    void attempt(action, appName);
  }

  function updateTheme(theme: Theme): void {
    ui.theme = theme;
    document.documentElement.dataset.theme = theme;
    void attempt(() => api.setTheme(theme));
  }

  function selectTab(tab: ShellTab): void {
    ui.tab = tab;
  }

  const intentTabs = ['home', 'apps', 'updates', 'shortcuts', 'learn', 'connections', 'engines', 'settings'];
  function openIntent(page: string | null): void {
    if (page === 'uninstall') {
      ui.reviewUninstall = true;
      selectTab('settings');
    } else if (page && intentTabs.includes(page)) {
      selectTab(page as ShellTab);
    }
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
    void ui.shortcutsIndex;
    void ui.shortcutLabels;
    void ui.shortcutsRevision;
    void ui.catalogData;
    void ui.catalogLoading;
    void ui.releaseNotesHtml;
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

    void (async () => {
      try {
        const handlers = await Promise.all([
          appEvents.operationProgress((progress) => receiveProgress(progress)),
          appEvents.installRequest((id) => receiveInstallRequest(id)),
          appEvents.registryChanged(() => { void attempt(refresh); }),
          appEvents.managerMessage((message) => setMessage(message, true)),
          appEvents.updatesChanged((updates) => { ui.updates = updates; }),
          appEvents.shortcutsChanged(() => { void api.shortcutsIndex().then((index) => { ui.shortcutsIndex = index; setShortcutsEnabled(true); }); }),
          appEvents.trayNavigation(openIntent),
        ]);
        if (disposed) handlers.forEach((stop) => stop());
        else unlisten = handlers;
        await initialize();
        initialized = true;
        installProposal = await invoke('self_install_proposal');
        hasSeenFocus = true;
        const [index, labels, catalogData] = await Promise.all([
          api.shortcutsIndex(), api.shortcutLabels(), api.catalogData(),
        ]);
        ui.shortcutsIndex = index;
        ui.shortcutLabels = labels;
        ui.catalogData = catalogData;
        setShortcutsEnabled(true);
        const trayIntent = await invoke<string | null>('tray_ui_intent');
        openIntent(trayIntent);
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

  async function refreshCatalog(): Promise<void> {
    ui.catalogLoading = true;
    try {
      ui.catalogData = await api.refreshCatalog();
      await refresh();
      setMessage(`Catalog refreshed (${ui.catalogData.catalog.updated}).`);
    } finally {
      ui.catalogLoading = false;
    }
  }

  async function checkCatalogLatest(id: string): Promise<void> {
    const app = ui.apps.find((candidate) => candidate.id === id);
    if (!app) throw new Error('This app is absent from the installed app list.');
    await checkUpdates(app);
    const release = ui.releases.get(id);
    if (release) {
      const html = await api.renderMarkdown(release.notes);
      ui.releaseNotesHtml = { ...ui.releaseNotesHtml, [id]: html };
    }
  }

  function shortcutsChanged(): void {
    ui.shortcutsRevision += 1;
  }

  async function persistShortcutLabel(key: string, label: string | null): Promise<void> {
    await api.setShortcutLabel(key, label);
    const next = { ...ui.shortcutLabels };
    const cleanLabel = label?.trim();
    if (cleanLabel) next[key] = cleanLabel;
    else delete next[key];
    ui.shortcutLabels = next;
    shortcutsChanged();
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
        onFix={(app, operation) => confirmOperation(app, operation)}
        {run}
      />
      <Apps
        active={ui.tab === 'apps'}
        apps={ui.apps}
        selected={ui.selected}
        releases={ui.releases}
        releaseNotesHtml={ui.releaseNotesHtml}
        catalogData={ui.catalogData}
        catalogLoading={ui.catalogLoading}
        shortcuts={ui.shortcutsIndex}
        busy={ui.busy}
        onChannel={channel}
        onLogin={login}
        onLaunch={launchApp}
        onInstall={install}
        onCheckUpdates={checkUpdates}
        onOperation={operation}
        onReleases={openReleases}
        onCatalogRefresh={refreshCatalog}
        onCheckLatest={checkCatalogLatest}
        onOpenDocs={openLearn}
        onInstallCatalogApp={(id) => reviewInstall(ui.apps.find((app) => app.id === id)!)}
        onUpdateCatalogApp={(id) => reviewUpdate(ui.apps.find((app) => app.id === id)!)}
        onUninstallCatalogApp={(id) => confirmOperation(ui.apps.find((app) => app.id === id)!, 'uninstall')}
        onInstallBundle={installBundle}
        {run}
      />
      <Updates
        active={ui.tab === 'updates'}
        apps={ui.toolsApp ? [ui.toolsApp, ...ui.apps] : ui.apps}
        updates={ui.updates}
        autoUpdate={ui.autoUpdate}
        busy={ui.busy}
        checking={ui.updateChecking}
        updateChecksEnabled={ui.updateChecksEnabled}
        onCheckNow={checkAllUpdates}
        onUpdate={(app) => app.id === 'arcade.tools' ? withBusy(api.selfUpdate, 'Updating Arcade Tools') : reviewUpdate(app)}
        onUpdateAll={updateAll}
        onRollback={(app) => confirmOperation(app, 'rollback')}
        onAutoUpdate={setAutoUpdate}
        onSelectApps={() => selectTab('apps')}
        {run}
      />
      {#if LearnView}<LearnView active={ui.tab === 'learn'} target={ui.learnTarget} {run} />{/if}
      {#if ConnectionsView}<ConnectionsView active={ui.tab === 'connections'} onSettings={() => selectTab('settings')} onLearn={openLearn} {run} />{/if}
      <Engines active={ui.tab === 'engines'} {run} />
      <Settings
        active={ui.tab === 'settings'}
        theme={ui.theme}
        linkEnabled={ui.linkEnabled}
        diagnostics={ui.diagnostics}
        busy={ui.busy}
        toolsApp={ui.toolsApp}
        apps={ui.apps}
        updateChecksEnabled={ui.updateChecksEnabled}
        notificationsEnabled={ui.notificationsEnabled}
        onUpdateChecks={setUpdateChecks}
        onOnboarding={() => selectTab('onboarding')}
        onTheme={updateTheme}
        onLink={setLink}
        {run}
      />
      <Onboarding
        active={ui.tab === 'onboarding'}
        apps={ui.apps}
        onInstall={install}
        onComplete={() => selectTab('home')}
        onSelectTab={selectTab}
        shortcuts={ui.shortcutsIndex}
        {run}
      />
      <Shortcuts
        active={ui.tab === 'shortcuts'}
        index={ui.shortcutsIndex}
        labels={ui.shortcutLabels}
        onLabel={persistShortcutLabel}
        onChange={shortcutsChanged}
      />
    </main>
    <footer class="shell-footer"><span>Arcade Tools · 0.2.1</span><span>Apps stay independent; Tools makes them easier to manage.</span></footer>
  </div>
</div>
<ProgressBar visible={ui.busy} phase={ui.phase} progress={ui.progress} cancellable={ui.cancellable} onCancel={() => run(cancelOperation)} />
<AppDialog />

<dialog bind:this={installDialog} aria-labelledby="self-install-title">
  {#if installProposal}<h2 id="self-install-title">{installProposal.title}</h2><p>{installProposal.copy}</p>
    {#if installProposal.options}<label><input type="checkbox" bind:checked={installLogin} /> Start at login</label>
    <label><input type="checkbox" bind:checked={removeDownload} /> Remove the downloaded file</label>{/if}
    <div class="dialog-actions"><button type="button" onclick={() => run(confirmSelfInstall)}>{installProposal.primary}</button><button type="button" onclick={() => { installProposal=null; installDialog.close(); }}>{installProposal.secondary}</button><button type="button" onclick={() => run(async () => { await invoke('dismiss_self_install'); installProposal=null; installDialog.close(); })}>Don't ask again for this file</button></div>
  {/if}
</dialog>
