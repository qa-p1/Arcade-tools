<script lang="ts">
  import { ui } from '../lib/state.svelte';
  import { api, type AppView, type Dashboard, type HealthIssue, type Operation, type UpdateState } from '../lib/api';
  import type { ShellTab } from '../lib/tabs.svelte';

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
  let healthChecked = $state(false);
  const installed = $derived(apps.filter((app) => app.installed));
  const running = $derived(installed.filter((app) => app.running));
  const updatesAvailable = $derived(apps.filter((app) => app.managed && updates?.apps[app.id]?.updateAvailable && !updates.apps[app.id]?.heldBack));
  const lastChecked = $derived(updates?.lastCheck ? new Date(updates.lastCheck * 1000).toLocaleString() : 'Not checked yet');
  const appName = (id: string) => apps.find((app) => app.id === id)?.name ?? id;
  const megabytes = (bytes: number | null) => bytes === null ? '—' : `${Math.round(bytes / (1024 * 1024))} MB`;
  const fixLabels: Record<HealthIssue['fixAction'], string> = {
    repair: 'Repair', adopt: 'Let Tools manage it', remove_stale_registry: 'Remove entry', remove_stale_receipt: 'Remove record',
    open_settings: 'Open settings', check_updates: 'Check for updates', open_engines: 'Open Engines', open_shortcuts: 'Open Shortcuts',
  };

  async function loadHealth(): Promise<void> {
    issues = await api.health();
    healthChecked = true;
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
    void loadHealth().catch(() => { healthChecked = false; });
    const timer = setInterval(sample, 2000);
    return () => { stopped = true; clearInterval(timer); };
  });
</script>

<div id="panel-home" class="tab-panel" role="tabpanel" aria-labelledby="tab-home" tabindex="0" hidden={!active}>
  <header class="page-heading">
    <div>
      <p class="eyebrow">ARCADE TOOLS / HOME</p>
      <h1>Your Arcade at a glance.</h1>
      <p class="intro">See what is installed, start the apps you use, and catch up on updates.</p>
    </div>
    <span class="app-count">{installed.length} installed</span>
  </header>

  <section class="home-metrics" aria-label="Arcade status">
    <div class="metric-card"><span>Installed</span><strong>{installed.length}</strong></div>
    <div class="metric-card"><span>Running</span><strong>{running.length}</strong></div>
    <button class="metric-card metric-link" type="button" onclick={() => onSelectTab('updates')}>
      <span>Available updates</span><strong>{updatesAvailable.length}</strong>
    </button>
    <div class="metric-card"><span>Memory in use</span><strong>{dashboard ? megabytes(dashboard.totalMemoryBytes) : '—'}</strong></div>
    <div class="metric-card"><span>Arcade CPU</span><strong>{dashboard?.totalCpuPercent.toFixed(1) ?? '—'}%</strong></div>
  </section>

  {#if updatesAvailable.length}<section class="feature-section" role="status"><strong>{updatesAvailable.length} updates available</strong><button type="button" onclick={() => onSelectTab('updates')}>Review updates</button></section>{/if}

  {#if issues.length}
    <section class="feature-section" aria-labelledby="home-health-heading">
      <div class="section-heading"><div><h2 id="home-health-heading">Needs attention</h2><p class="setting-description">Tools found {issues.length} {issues.length === 1 ? 'problem' : 'problems'} it can help with.</p></div><button type="button" disabled={busy} onclick={() => run(loadHealth)}>Check again</button></div>
      <div class="home-app-list">
        {#each issues as issue (`${issue.app}:${issue.kind}:${issue.path ?? ''}:${issue.reason}`)}
          <div class="home-app-row">
            <div class="home-app-info"><strong>{appName(issue.app)}</strong><span>{issue.reason}</span></div>
            {#if issue.canFix}<button type="button" class="primary-action" disabled={busy} onclick={() => run(() => fix(issue), appName(issue.app))}>{fixLabels[issue.fixAction]}</button>{/if}
          </div>
        {/each}
      </div>
    </section>
  {:else if healthChecked}
    <p class="setting-description home-health-ok">Health check: everything looks fine.</p>
  {/if}

  <section class="feature-section" aria-labelledby="home-actions-heading">
    <div class="section-heading"><div><h2 id="home-actions-heading">Quick actions</h2><p class="setting-description">Pick up where you left off.</p></div></div>
    <div class="quick-actions">
      <button type="button" class="primary-action" disabled={busy || installed.length === 0} onclick={() => run(onStartAll, 'Arcade apps')}>Open all stopped apps</button>
      <button type="button" disabled={busy} onclick={() => onSelectTab('apps')}>Browse apps</button>
      <button type="button" disabled={busy} onclick={() => run(onCheckNow)}>Check for updates</button>
    </div>
    <p class="last-checked">Last update check: {lastChecked}</p>
  </section>

  <section class="feature-section" aria-labelledby="home-installed-heading">
    <div class="section-heading"><div><h2 id="home-installed-heading">Installed apps</h2><p class="setting-description">Open an app or manage it from the Apps tab.</p></div></div>
    {#if installed.length}
      <div class="home-app-list">
        {#each installed as app (app.id)}
          {@const usage = dashboard?.apps.find((entry) => entry.id === app.id)}
          <div class="home-app-row">
            <img class="app-glyph" src="/glyphs/{app.id}.svg" alt="" width="30" height="30" />
            <div class="home-app-info"><strong>{app.name}</strong><span>{app.running ? 'Running' : 'Installed'}{app.version ? ` · v${app.version}` : ''} · {app.channel}{usage?.pid ? ` · ${megabytes(usage.memoryBytes)} · ${usage.cpuPercent?.toFixed(1) ?? '—'}% CPU · ${Math.floor((usage.uptimeSeconds ?? 0) / 60)} min` : ''}{usage?.tray ? ` · ${usage.tray} tray` : ''} · {usage ? (usage.linkEnabled ? 'Link enabled' : 'Link disabled') : 'Link status unknown'}</span></div>
            <button type="button" class="primary-action" disabled={busy} onclick={() => run(() => onOpen(app), app.name)}>{app.running ? 'Open' : 'Start'}</button>
            <button type="button" disabled={busy} onclick={() => run(() => api.openAppSettings(app.id), app.name)}>Settings</button>
            {#if app.running}<button type="button" disabled={busy} onclick={() => run(() => api.restartApp(app.id), app.name)}>Restart</button>{/if}
          </div>
        {/each}
      </div>
    {:else if !ui.loaded}
      <p class="setting-description">Loading your apps…</p>
    {:else}
      <div class="empty-state"><h3>Arcade starts here</h3><p>Choose an app or a bundle in Onboarding. Nothing installs until you review and confirm it.</p><button type="button" class="primary-action" onclick={() => onSelectTab('apps')}>Browse apps</button></div>
    {/if}
  </section>
</div>
