<script lang="ts">
  import AppCard from '../components/AppCard.svelte';
  import type { AppView, Channel, Operation, Release } from '../lib/api';

  type Props = {
    active: boolean;
    apps: AppView[];
    selected: string | null;
    releases: Map<string, Release>;
    busy: boolean;
    onChannel: (app: AppView, channel: Channel) => Promise<void>;
    onLogin: (app: AppView, enabled: boolean) => Promise<void>;
    onLaunch: (app: AppView, mode: 'foreground' | 'background') => Promise<void>;
    onInstall: (app: AppView) => Promise<void>;
    onCheckUpdates: (app: AppView) => Promise<void>;
    onOperation: (app: AppView, operation: Operation) => Promise<void>;
    onReleases: (app: AppView) => Promise<void>;
    run: (action: () => Promise<void>, appName: string) => void;
  };

  let {
    active, apps, selected, releases, busy, onChannel, onLogin, onLaunch, onInstall,
    onCheckUpdates, onOperation, onReleases, run,
  }: Props = $props();
</script>

<div id="panel-apps" class="tab-panel" role="tabpanel" aria-labelledby="tab-apps" tabindex="0" hidden={!active}>
  <header class="page-heading">
    <div>
      <p class="eyebrow">ARCADE TOOLS / APPS</p>
      <h1>Your apps, in one place.</h1>
      <p class="intro">Install, update and take care of your Arcade apps.</p>
    </div>
    <span class="app-count">{apps.length} apps</span>
  </header>
  <div id="apps" class="app-list" aria-label="Arcade apps">
    {#each apps as app (app.id)}
      <AppCard
        {app}
        selected={selected === app.id}
        release={releases.get(app.id)}
        {busy}
        {onChannel}
        {onLogin}
        {onLaunch}
        {onInstall}
        {onCheckUpdates}
        {onOperation}
        {onReleases}
        {run}
      />
    {/each}
  </div>
</div>
