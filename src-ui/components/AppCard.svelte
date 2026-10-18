<script lang="ts">
  import type { AppView, Channel, Operation, Release } from '../lib/api';

  type Props = {
    app: AppView;
    selected: boolean;
    release?: Release;
    busy: boolean;
    onChannel: (app: AppView, channel: Channel) => Promise<void>;
    onLogin: (app: AppView, enabled: boolean) => Promise<void>;
    onLaunch: (app: AppView, mode: 'foreground' | 'background') => Promise<void>;
    onInstall: (app: AppView) => Promise<void>;
    onCheckUpdates: (app: AppView) => Promise<void>;
    onOperation: (app: AppView, operation: Operation) => Promise<void>;
    onReleases: (app: AppView) => Promise<void>;
    onDetails: (app: AppView) => void;
    run: (action: () => Promise<void>, appName: string) => void;
  };

  let {
    app, selected, release, busy, onChannel, onLogin, onLaunch, onInstall, onCheckUpdates,
    onOperation, onReleases, onDetails, run,
  }: Props = $props();

  const status = $derived(app.running ? 'Running' : app.installed ? 'Installed' : app.managed ? 'Needs repair' : 'Not installed');
</script>

<article id={app.id} class:selected aria-label={app.name}>
  <img class="app-glyph" src="/glyphs/{app.id}.svg" alt="" width="30" height="30" />
  <div class="info">
    <div class="card-heading">
      <h2>{app.name}</h2>
      <span class="badge">{status}</span>
    </div>
    <p class="pitch">{app.pitch}</p>
    <div class="controls">
      <label>Channel
        <select aria-label="{app.name} channel" value={app.channel} disabled={busy || (app.installed && !app.managed)} onchange={(event) => run(() => onChannel(app, event.currentTarget.value as Channel), app.name)}>
          <option value="stable">Stable</option>
          <option value="nightly">Nightly</option>
        </select>
      </label>
      {#if app.version}<span class="version">v{app.version}</span>{/if}
      {#if app.installed && app.loginSupported}
        <label class="login"><input type="checkbox" checked={app.startAtLogin} disabled={busy} aria-label="{app.name} start at login" onchange={(event) => run(() => onLogin(app, event.currentTarget.checked), app.name)} /> Start at login</label>
      {:else if app.installed}
        <span class="version">Start at login: use the app’s settings</span>
      {/if}
    </div>
    {#if app.reason}<p class="reason">{app.reason}</p>{/if}
  </div>
  <div class="actions" aria-label="{app.name} actions">
    {#if app.installed}
      <button class="primary" type="button" disabled={busy} onclick={() => run(() => onLaunch(app, 'foreground'), app.name)}>Open</button>
      <button type="button" disabled={busy} onclick={() => run(() => onLaunch(app, 'background'), app.name)}>Background</button>
    {/if}
    {#if app.installed && !app.managed && app.installationClass !== 'dev'}
      <button class="primary" type="button" disabled={busy} onclick={() => run(() => onOperation(app, 'adopt'), app.name)}>Let Tools manage it</button>
    {/if}
    {#if !app.installed && !app.managed}
      <button class="primary" type="button" disabled={busy} onclick={() => run(() => onInstall(app), app.name)}>Check &amp; install</button>
    {/if}
    {#if app.managed}
      <button type="button" disabled={busy} onclick={() => run(() => onCheckUpdates(app), app.name)}>Check updates</button>
      {#if release?.updateAvailable}<button class="primary" type="button" disabled={busy} onclick={() => run(() => onOperation(app, 'update'), app.name)}>Update</button>{/if}
      <button type="button" disabled={busy} onclick={() => run(() => onOperation(app, 'repair'), app.name)}>Repair</button>
      <button class="quiet danger" type="button" disabled={busy} onclick={() => run(() => onOperation(app, 'uninstall'), app.name)}>Remove</button>
    {/if}
    <button class="quiet" type="button" disabled={busy} onclick={() => run(() => onReleases(app), app.name)}>Releases ↗</button>
    <button class="quiet" type="button" disabled={busy} onclick={() => onDetails(app)}>Details</button>
  </div>
</article>
