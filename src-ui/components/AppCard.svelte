<script lang="ts">
  import { glyph } from '../lib/glyphs';
  import type { AppView, Channel, Operation, Release } from '../lib/api';
  import Icon from './Icon.svelte';
  import { tooltip } from '../lib/tooltip';

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
    onOperation, onDetails, run,
  }: Props = $props();

  const status = $derived(app.running ? 'Running' : app.installed ? 'Installed' : app.managed ? 'Needs repair' : null);
  const tone = $derived(app.running ? 'ok' : app.managed && !app.installed ? 'warn' : '');
</script>

<article id={app.id} class="card app-card" class:selected aria-label={app.name}>
  <div class="app-card-head">
    <span class="glyph-tile"><img src={glyph(app.id)} alt="" width="18" height="18" /></span>
    <div class="row-main">
      <h2><button type="button" class="title-button" disabled={busy} onclick={() => onDetails(app)}>{app.name}</button></h2>
      <p class="pitch">{app.tagline}</p>
    </div>
    {#if release?.updateAvailable}<span class="badge ok">Update {release.version}</span>{:else if status}<span class="badge {tone}">{#if tone}<span class="dot {tone}"></span>{/if}{status}</span>{/if}
  </div>
  {#if app.reason}<p class="reason">{app.reason}</p>{/if}
  <div class="app-card-meta">
    <select class="select sm" aria-label="{app.name} channel" value={app.channel} disabled={busy || (app.installed && !app.managed)} onchange={(event) => run(() => onChannel(app, event.currentTarget.value as Channel), app.name)}>
      <option value="stable">Stable</option>
      <option value="nightly">Nightly</option>
    </select>
    {#if app.version}<span class="faint">v{app.version}</span>{/if}
    <span class="spacer"></span>
    {#if app.installed && app.loginSupported}
      <label class="check-row login"><span>Start at login</span><input class="switch" type="checkbox" checked={app.startAtLogin} disabled={busy} aria-label="{app.name} start at login" onchange={(event) => run(() => onLogin(app, event.currentTarget.checked), app.name)} /></label>
    {/if}
  </div>
  <div class="app-card-actions" aria-label="{app.name} actions">
    {#if app.installed}
      <button class="btn sm primary" type="button" disabled={busy} onclick={() => run(() => onLaunch(app, 'foreground'), app.name)}>Open</button>
    {/if}
    {#if release?.updateAvailable && app.managed}<button class="btn sm primary" type="button" disabled={busy} onclick={() => run(() => onOperation(app, 'update'), app.name)}>Update</button>{/if}
    {#if app.installed && !app.managed && app.installationClass !== 'dev'}
      <button class="btn sm" type="button" disabled={busy} onclick={() => run(() => onOperation(app, 'adopt'), app.name)}>Let Tools manage it</button>
    {/if}
    {#if !app.installed && !app.managed}
      <button class="btn sm primary" type="button" disabled={busy} onclick={() => run(() => onInstall(app), app.name)}>Check &amp; install</button>
    {/if}
    <span class="spacer"></span>
    {#if app.managed}
      <button class="btn ghost icon-only sm" type="button" aria-label="Check for updates" use:tooltip={{ text: 'Check for updates' }} disabled={busy} onclick={() => run(() => onCheckUpdates(app), app.name)}><Icon name="refresh" size={15} /></button>
      <button class="btn ghost icon-only sm" type="button" title="Repair" aria-label="Repair" disabled={busy} onclick={() => run(() => onOperation(app, 'repair'), app.name)}><Icon name="wrench" size={15} /></button>
      <button class="btn ghost icon-only sm danger" type="button" title="Remove" aria-label="Remove" disabled={busy} onclick={() => run(() => onOperation(app, 'uninstall'), app.name)}><Icon name="trash" size={15} /></button>
    {/if}
    <button class="btn ghost sm" type="button" aria-label="Details" disabled={busy} onclick={() => onDetails(app)}>Details<Icon name="chevronRight" size={14} /></button>
  </div>
</article>

<style>
  .app-card { display: flex; flex-direction: column; gap: 12px; min-width: 0; padding: 14px 14px 10px; }
  .app-card.selected { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .app-card-head { display: flex; align-items: flex-start; gap: 12px; }
  h2 { font-size: var(--fs-heading); font-weight: 600; }
  .title-button { padding: 0; border: 0; background: none; color: var(--text); font: inherit; cursor: pointer; }
  .title-button:hover { text-decoration: underline; }
  .pitch { display: -webkit-box; margin-top: 2px; overflow: hidden; color: var(--muted); font-size: var(--fs-control); -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  .reason { padding: 8px 10px; border-radius: var(--radius-sm); background: var(--sunken); color: var(--muted); font-size: var(--fs-small); }
  .app-card-meta { display: flex; align-items: center; gap: 10px; margin-top: auto; font-size: var(--fs-small); }
  .login { color: var(--muted); }
  .app-card-actions { display: flex; align-items: center; gap: 2px; margin: 0 -4px; padding-top: 10px; border-top: 1px solid var(--line); }
  .app-card-actions > .btn:not(.ghost) { margin-right: 4px; }
  .spacer { flex: 1; }
</style>
