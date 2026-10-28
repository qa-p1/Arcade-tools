<script lang="ts">
  import { formatSize } from '../design/format';
  import { relativeInSentence } from '../lib/format';
  import { glyph } from '../lib/glyphs';
  import { ui } from '../lib/state.svelte';
  import { refresh } from '../lib/controller';
  import { api, type AppView, type Dashboard, type HealthIssue, type Operation, type UpdateState } from '../lib/api';
  import type { ShellTab } from '../lib/tabs.svelte';
  import Icon from '../components/Icon.svelte';

  type Props = {
    active: boolean;
    apps: AppView[];
    updates: UpdateState | null;
    busy: boolean;
    onOpen: (app: AppView) => Promise<void>;
    onStartAll: () => Promise<void>;
    onCheckNow: () => Promise<void>;
    onSelectTab: (tab: ShellTab) => void;
    onFix: (app: AppView, operation: Operation) => Promise<void>;
    run: (action: () => Promise<void>, label?: string) => void;
  };
  let { active, apps, updates, busy, onOpen, onStartAll, onCheckNow, onSelectTab, onFix, run }: Props = $props();
  let dashboard = $state<Dashboard | null>(null);
  let issues = $state<HealthIssue[]>([]);
  const installed = $derived(apps.filter((app) => app.installed));
  const running = $derived(installed.filter((app) => app.running));
  const updatesAvailable = $derived(apps.filter((app) => app.managed && updates?.apps[app.id]?.updateAvailable && !updates.apps[app.id]?.heldBack));
  const lastChecked = $derived(updates?.lastCheck ? relativeInSentence(updates.lastCheck * 1000) : null);
  const stopped = $derived(installed.filter((app) => !app.running));
  let showIssues = $state(false);
  const appName = (id: string) => apps.find((app) => app.id === id)?.name ?? id;
  const fixLabels: Record<HealthIssue['fixAction'], string> = {
    repair: 'Repair', adopt: 'Let Tools manage it', remove_stale_registry: 'Remove entry', remove_stale_receipt: 'Remove record',
    open_settings: 'Open settings', check_updates: 'Check for updates', open_engines: 'Open Engines', open_shortcuts: 'Open Shortcuts',
  };

  async function loadHealth(): Promise<void> {
    issues = await api.health();
  }

  async function fix(issue: HealthIssue): Promise<void> {
    const app = apps.find((candidate) => candidate.id === issue.app);
    switch (issue.fixAction) {
      case 'repair':
      case 'adopt':
        if (app) await onFix(app, issue.fixAction);
        else await api.fixHealth(issue.app, issue.fixAction);
        break;
      case 'open_settings': await api.openAppSettings(issue.app); break;
      case 'check_updates': await onCheckNow(); break;
      case 'open_engines': onSelectTab('engines'); return;
      case 'open_shortcuts': onSelectTab('shortcuts'); return;
      default: await api.fixHealth(issue.app, issue.fixAction);
    }
    await loadHealth();
  }

  // Usage is sampled while Home is visible; health is checked when it opens.
  $effect(() => {
    if (!active) return;
    let stopped = false;
    const sample = async () => { try { const next = await api.dashboard(); if (!stopped) dashboard = next; } catch { /* keep the last sample */ } };
    void sample();
    void loadHealth().catch(() => undefined);
    const timer = setInterval(sample, 2000);
    return () => { stopped = true; clearInterval(timer); };
  });
</script>

<div id="panel-home" class="tab-panel" role="tabpanel" aria-labelledby="tab-home" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Home</h1>
      <p class="sub">
        {ui.loaded ? `${installed.length} installed · ${running.length} running${dashboard?.totalMemoryBytes ? ` · ${formatSize(dashboard.totalMemoryBytes)} in use` : ''}` : 'Loading your apps…'}
      </p>
    </div>
    <div class="page-actions">
      <button type="button" class="btn" disabled={busy} onclick={() => run(onCheckNow)}><Icon name="refresh" size={14} />Check for updates</button>
      <button type="button" class="btn primary" disabled={busy || stopped.length === 0} onclick={() => run(onStartAll, 'Arcade apps')}><Icon name="play" size={13} />Open all stopped apps</button>
    </div>
  </header>

  {#if updatesAvailable.length}
    <div class="notice ok" role="status">
      <Icon name="updates" />
      <span class="row-main">{updatesAvailable.length === 1 ? `An update is ready for ${updatesAvailable[0].name}` : `${updatesAvailable.length} updates are ready`}</span>
      <button type="button" class="btn sm primary" onclick={() => onSelectTab('updates')}>Review</button>
    </div>
  {/if}

  {#if issues.length}
    <section class="home-issues" aria-labelledby="home-health-heading">
      <div class="notice warn">
        <Icon name="alert" />
        <span class="row-main" id="home-health-heading">{issues.length === 1 ? '1 thing needs attention' : `${issues.length} things need attention`}</span>
        {#if showIssues}<button type="button" class="btn sm ghost" disabled={busy} onclick={() => run(loadHealth)}>Check again</button>{/if}
        <button type="button" class="btn sm ghost" aria-expanded={showIssues} onclick={() => showIssues = !showIssues}>{showIssues ? 'Hide' : 'Show'}</button>
      </div>
      {#if showIssues}
        <div class="list issue-list">
          {#each issues as issue (`${issue.app}:${issue.kind}:${issue.path ?? ''}:${issue.reason}`)}
            <div class="row">
              <div class="row-main"><div class="row-title">{appName(issue.app)}</div><div class="row-sub">{issue.reason}</div></div>
              {#if issue.canFix}<button type="button" class="btn sm" disabled={busy} onclick={() => run(() => fix(issue), appName(issue.app))}>{fixLabels[issue.fixAction]}</button>{/if}
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  <section class="section" aria-labelledby="home-installed-heading">
    <div class="section-head"><h2 id="home-installed-heading">Your apps</h2>{#if lastChecked}<span class="meta">Updates checked {lastChecked}</span>{/if}</div>
    {#if installed.length}
      <div class="home-grid">
        {#each installed as app (app.id)}
          {@const usage = dashboard?.apps.find((entry) => entry.id === app.id)}
          <div class="card home-tile">
            <div class="home-tile-top">
              <span class="glyph-tile"><img src={glyph(app.id)} alt="" width="18" height="18" /></span>
              <div class="row-main">
                <div class="row-title">{app.name}</div>
                <div class="row-sub"><span class="dot" class:ok={app.running}></span> {app.running ? 'Running' : 'Not running'}{app.running && usage?.memoryBytes ? ` · ${formatSize(usage.memoryBytes)}` : ''}</div>
              </div>
            </div>
            <div class="home-tile-actions">
              <button type="button" class="btn sm" class:primary={!app.running} disabled={busy} onclick={() => run(() => onOpen(app), app.name)}>{app.running ? 'Open' : 'Start'}</button>
              <span class="spacer"></span>
              {#if app.running}<button type="button" class="btn ghost icon-only sm" title="Restart" aria-label="Restart {app.name}" disabled={busy} onclick={() => run(() => api.restartApp(app.id), app.name)}><Icon name="restart" size={15} /></button>{/if}
              <button type="button" class="btn ghost icon-only sm" title="Settings" aria-label="{app.name} settings" disabled={busy} onclick={() => run(() => api.openAppSettings(app.id), app.name)}><Icon name="settings" size={15} /></button>
            </div>
          </div>
        {/each}
      </div>
    {:else if !ui.loaded && ui.loadError}
      <div class="notice danger" role="alert"><Icon name="alert" /><span class="row-main">Couldn't list your apps. {ui.loadError}</span><button type="button" class="btn sm" onclick={() => run(refresh)}>Retry</button></div>
    {:else if !ui.loaded}
      <div class="home-grid">{#each [0, 1, 2] as item (item)}<div class="card home-tile"><div class="home-tile-top"><span class="glyph-tile skeleton"></span><div class="row-main"><div class="skeleton line home-skel-a"></div><div class="skeleton line home-skel-b"></div></div></div></div>{/each}</div>
    {:else}
      <div class="empty-state">
        <Icon name="apps" size={24} />
        <h3>No Arcade apps yet</h3>
        <p>Pick an app from the catalog. Nothing installs until you review and confirm it.</p>
        <div class="btn-group"><button type="button" class="btn primary" onclick={() => onSelectTab('apps')}>Browse apps</button></div>
      </div>
    {/if}
  </section>
</div>

<style>
  .home-issues { margin-top: 12px; }
  .issue-list { margin-top: 8px; }
  .home-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 12px; }
  .home-tile { display: grid; gap: 14px; padding: 14px; }
  .home-tile + .home-tile { margin-top: 0; }
  .home-tile-top { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .home-tile .row-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .home-tile .row-sub { display: flex; align-items: center; gap: 6px; white-space: nowrap; }
  .home-tile-actions { display: flex; align-items: center; gap: 4px; }
  .spacer { flex: 1; }
  .home-skel-a { width: 60%; }
  .home-skel-b { width: 40%; margin-top: 8px; }
</style>
