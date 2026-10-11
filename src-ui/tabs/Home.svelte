<script lang="ts">
  import type { AppView, UpdateState } from '../lib/api';

  type Props = {
    active: boolean;
    apps: AppView[];
    updates: UpdateState | null;
    busy: boolean;
    onOpen: (app: AppView) => Promise<void>;
    onStartAll: () => Promise<void>;
    onCheckNow: () => Promise<void>;
    onSelectTab: (tab: 'apps' | 'updates') => void;
    run: (action: () => Promise<void>, label?: string) => void;
  };
  let { active, apps, updates, busy, onOpen, onStartAll, onCheckNow, onSelectTab, run }: Props = $props();
  const installed = $derived(apps.filter((app) => app.installed));
  const running = $derived(installed.filter((app) => app.running));
  const updatesAvailable = $derived(apps.filter((app) => app.managed && updates?.apps[app.id]?.updateAvailable && !updates.apps[app.id]?.heldBack));
  const lastChecked = $derived(updates?.lastCheck ? new Date(updates.lastCheck * 1000).toLocaleString() : 'Not checked yet');
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
  </section>

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
          <div class="home-app-row">
            <img class="app-glyph" src="/glyphs/{app.id}.svg" alt="" width="30" height="30" />
            <div class="home-app-info"><strong>{app.name}</strong><span>{app.running ? 'Running' : 'Installed'}{app.version ? ` · v${app.version}` : ''}</span></div>
            <button type="button" class="primary-action" disabled={busy} onclick={() => run(() => onOpen(app), app.name)}>{app.running ? 'Open' : 'Start'}</button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-state"><h3>Arcade starts here</h3><p>Choose an app or a bundle in Onboarding. Nothing installs until you review and confirm it.</p><button type="button" class="primary-action" onclick={() => onSelectTab('apps')}>Browse apps</button></div>
    {/if}
  </section>
</div>
