<script lang="ts">
  import { confirmOperation, changeChannel, changeLogin, launchApp } from '../lib/controller';
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import AppShortcutRow from './AppShortcutRow.svelte';
  import KeyChips from './KeyChips.svelte';
  import { api, type AppView, type Release, type ConnectionPeer } from '../lib/api';
  import type { CatalogApp, Engine } from '../lib/catalog';
  import type { ShortcutsIndex } from '../lib/shortcuts';

  type CatalogAction = 'install' | 'update' | 'uninstall';
  type Props = {
    app: CatalogApp;
    installedApp?: AppView;
    latest?: Release;
    releaseNotesHtml?: string;
    shortcuts: ShortcutsIndex | null;
    engines: Engine[];
    busy: boolean;
    onBack: () => void;
    onCheckLatest: () => Promise<void>;
    onOpenDocs: () => Promise<void>;
    onInstall: (id: string) => Promise<void>;
    onUpdate: (id: string) => Promise<void>;
    onUninstall: (id: string) => Promise<void>;
    run: (action: () => Promise<void>, appName: string) => void;
  };

  let {
    app, installedApp, latest, releaseNotesHtml, shortcuts, engines, busy,
    onBack, onCheckLatest, onOpenDocs, onInstall, onUpdate, onUninstall, run,
  }: Props = $props();

  let history = $state<{ version: string; publishedAt: string; prerelease: boolean; html: string }[]>([]);
  let peers = $state<ConnectionPeer[]>([]);
  $effect(() => { void app.id; history = []; void api.listConnections().then((view) => peers = view.peers).catch(() => peers = []); });
  const offeredActions = $derived(peers.find((peer) => peer.id === app.id)?.actions ?? []);
  async function loadHistory(): Promise<void> { history=await api.releaseHistory(app.id); }
  let ownIcon = $state(false);
  $effect(() => { const id=app.id; void invoke<{ preferences: { excluded: string[] } }>('tray_get_settings').then((view) => ownIcon=view.preferences.excluded.includes(id)).catch(() => {}); });
  async function setOwnIcon(enabled: boolean): Promise<void> {
    const view = await invoke<{ preferences: { excluded: string[] } }>('tray_get_settings');
    const excluded = view.preferences.excluded.filter((id) => id !== app.id);
    if (enabled) excluded.push(app.id);
    await invoke('tray_set_settings', { preferences: { ...view.preferences, excluded } });
    ownIcon = enabled;
  }
  let actionDialog: HTMLDialogElement;
  let pendingAction = $state<CatalogAction | null>(null);
  const installed = $derived(Boolean(installedApp?.installed || installedApp?.managed || installedApp?.version));
  const installReason = $derived(app.installReason ?? installedApp?.installReason ?? null);
  const shortcutSheet = $derived(shortcuts?.apps.find((sheet) => sheet.id === app.id || sheet.name === app.name));
  const arcadeShortcuts = $derived(shortcuts?.arcade.filter((shortcut) => shortcut.app === app.id) ?? []);
  const appEngines = $derived(engines.filter((engine) => engine.usedBy.some((use) => use.app === app.id)));
  const screenshots = $derived(app.screenshots.flatMap((source) => {
    try {
      const parsed = new URL(source);
      return parsed.protocol === 'https:' && parsed.hostname === 'raw.githubusercontent.com' ? [source] : [];
    } catch {
      return [];
    }
  }));

  async function askToConfirm(action: CatalogAction): Promise<void> {
    pendingAction = action;
    await tick();
    if (!actionDialog.open) actionDialog.showModal();
  }

  function confirmAction(): void {
    const action = pendingAction;
    pendingAction = null;
    actionDialog.close();
    if (!action) return;
    const operation = action === 'install' ? onInstall : action === 'update' ? onUpdate : onUninstall;
    run(() => operation(app.id), app.name);
  }

  function closeActionDialog(): void {
    pendingAction = null;
  }
</script>

<section class="catalog-detail" aria-labelledby="catalog-detail-title">
  <button class="back-button" type="button" onclick={onBack}>← Back to apps</button>
  <header class="detail-heading">
    <img class="detail-glyph" src="/glyphs/{app.id}.svg" alt="" width="44" height="44" />
    <div class="detail-heading-copy">
      <p class="eyebrow">ARCADE CATALOG / {app.category.toUpperCase()}</p>
      <h1 id="catalog-detail-title">{app.name}</h1>
      <p class="intro">{app.pitch}</p>
    </div>
    <span class="detail-status">{installed ? 'Installed' : 'Available'}</span>
  </header>

  <section class="detail-card detail-summary" aria-labelledby="detail-about-title">
    <div class="detail-copy">
      <h2 id="detail-about-title">About</h2>
      <p>{app.description}</p>
    </div>
    <dl class="version-comparison">
      <div><dt>Installed version</dt><dd>{installedApp?.version ?? 'Not installed'}</dd></div>
      <div><dt>Latest version</dt><dd>{latest?.version ?? 'Not checked'}</dd></div>
      {#if installReason}<div class="version-note"><dt>Install status</dt><dd>{installReason}</dd></div>{/if}
    </dl>
  </section>

  {#if screenshots.length}
    <section class="detail-section" aria-labelledby="detail-screenshots-title">
      <div class="detail-section-heading"><h2 id="detail-screenshots-title">Screenshots</h2><span>{screenshots.length}</span></div>
      <div class="screenshot-grid">
        {#each screenshots as source, index (`${source}-${index}`)}
          <img src={source} alt="{app.name} screenshot {index + 1}" loading="lazy" />
        {/each}
      </div>
    </section>
  {/if}

  <section class="detail-card detail-versions" aria-labelledby="detail-version-title">
    <div><h2 id="detail-version-title">Versions and releases</h2><p>Compare the installed build with the latest release on your selected channel.</p></div>
    <button class="small-button" type="button" disabled={busy} onclick={() => run(onCheckLatest, app.name)}>Check latest</button>
  </section>
  {#if latest?.notes}
    <section class="detail-card release-notes" aria-labelledby="detail-release-notes-title">
      <h2 id="detail-release-notes-title">Latest release notes · {latest.version}</h2>
      {#if releaseNotesHtml}<div class="release-markdown">{@html releaseNotesHtml}</div>{:else}<p>Preparing sanitized release notes…</p>{/if}
    </section>
  {/if}

  <section class="detail-section" aria-label="Release history">
    <button type="button" disabled={busy} onclick={() => run(loadHistory, app.name)}>Load version history</button>
    {#each history as release (`${release.version}-${release.publishedAt}`)}<details><summary>{release.version} · {release.publishedAt}{release.prerelease ? ' · preview' : ''}</summary><div class="markdown-body">{@html release.html}</div></details>{/each}
  </section>
  <section class="detail-section" aria-labelledby="detail-platforms-title">
    <div class="detail-section-heading"><h2 id="detail-platforms-title">Platform status</h2></div>
    <div class="platform-list">
      {#each Object.entries(app.platforms) as [platform, status] (platform)}
        <div><span>{platform}</span><strong>{status}</strong></div>
      {/each}
    </div>
  </section>

  <section class="detail-card detail-links" aria-label="App links">
    <div><h2>Documentation and releases</h2><p>Open guides and release history from the app repository.</p></div>
    <div class="detail-link-actions">
      <button class="small-button" type="button" onclick={() => run(onOpenDocs, app.name)}>Open documentation ↗</button>
      <a class="small-button link-button" href={app.homepage} target="_blank" rel="noreferrer">Repository ↗</a>
    </div>
  </section>

  <section class="detail-section" aria-labelledby="detail-shortcuts-title">
    <div class="detail-section-heading"><h2 id="detail-shortcuts-title">Shortcuts</h2></div>
    {#if shortcutSheet || arcadeShortcuts.length}
      {#if shortcutSheet}
        {#each shortcutSheet.groups as group (group.title)}
          <div class="detail-shortcut-group">
            <h3>{group.title}</h3>
            <ul class="shortcut-list">
              {#each group.shortcuts as shortcut (shortcut.id)}<AppShortcutRow {shortcut} os={shortcuts?.os ?? 'linux'} />{/each}
            </ul>
          </div>
        {/each}
      {/if}
      {#if arcadeShortcuts.length}
        <ul class="shortcut-list detail-global-shortcuts">
          {#each arcadeShortcuts as shortcut (`${shortcut.app}-${shortcut.id}`)}
            <li class="shortcut-row">
              <div class="shortcut-copy"><h3>{shortcut.title}</h3><p>Arcade global shortcut</p></div>
              <KeyChips keys={shortcut.display} os={shortcuts?.os ?? 'linux'} />
            </li>
          {/each}
        </ul>
      {/if}
    {:else}
      <p class="catalog-empty-note">No shortcut sheet is indexed for this app yet.</p>
    {/if}
  </section>

  <section class="detail-section" aria-labelledby="detail-engines-title">
    <div class="detail-section-heading"><h2 id="detail-engines-title">Required engines</h2></div>
    {#if appEngines.length}
      <div class="engine-list">
        {#each appEngines as engine (engine.id)}
          <article class="engine-card"><div><h3>{engine.name}</h3><p>{engine.usedBy.find((use) => use.app === app.id)?.purpose}</p></div><a href={engine.homepage} target="_blank" rel="noreferrer">Engine details ↗</a></article>
        {/each}
      </div>
    {:else}
      <p class="catalog-empty-note">This app does not list any shared engines.</p>
    {/if}
  </section>

  <section class="detail-section" aria-labelledby="detail-connections-title">
    <h2 id="detail-connections-title">What it adds to your other apps</h2>
    {#if offeredActions.length}<ul>{#each offeredActions as action (action.id)}<li><strong>{action.title}</strong>{action.accepts.length ? ` · accepts ${action.accepts.join(', ')}` : ''}{action.available ? '' : ` · ${action.reason ?? 'Currently unavailable'}`}</li>{/each}</ul>{:else}<p class="setting-description">Start this app to see the actions it advertises. Its guide explains how it works with other Arcade apps.</p>{/if}
    <button type="button" onclick={() => run(onOpenDocs, app.name)}>Read the app guide</button>
  </section>

  {#if installedApp && installed}
    <section class="detail-card" aria-label="Installed app management">
      <div class="actions-inline">
        <button type="button" disabled={busy} onclick={() => run(() => launchApp(installedApp!, 'foreground'), app.name)}>Open</button>
        <button type="button" disabled={busy} onclick={() => run(() => api.openAppSettings(app.id), app.name)}>Open settings</button>
        {#if installedApp.managed}<button type="button" disabled={busy} onclick={() => run(() => confirmOperation(installedApp!, 'repair'), app.name)}>Repair</button>{/if}
        {#if !installedApp.managed && installedApp.installationClass !== 'dev'}<button type="button" disabled={busy} onclick={() => run(() => confirmOperation(installedApp!, 'adopt'), app.name)}>Let Tools manage it</button>{/if}
        {#if installedApp.rollbackVersion}<button type="button" disabled={busy} onclick={() => run(() => confirmOperation(installedApp!, 'rollback'), app.name)}>Roll back to {installedApp.rollbackVersion}</button>{/if}
      </div>
      <label>Release channel <select value={installedApp.channel} disabled={busy} onchange={(event) => run(() => changeChannel(installedApp!, event.currentTarget.value as 'stable' | 'nightly'), app.name)}><option value="stable">Stable</option><option value="nightly">Nightly</option></select></label>
      {#if installedApp.loginSupported}<label><input type="checkbox" checked={installedApp.startAtLogin} disabled={busy} onchange={(event) => run(() => changeLogin(installedApp!, event.currentTarget.checked), app.name)} /> Start at login</label>{/if}
      <label><input type="checkbox" checked={ownIcon} disabled={busy} onchange={(event) => run(() => setOwnIcon(event.currentTarget.checked), app.name)} /> Keep its own tray icon</label>
    </section>
  {/if}
  <section class="detail-actions" aria-label="Catalog app actions">
    {#if !installed}
      <button class="primary" type="button" disabled={busy || Boolean(installReason) || !installedApp?.installSupported} onclick={() => askToConfirm('install')}>Install</button>
    {:else}
      <button class="primary" type="button" disabled={busy || Boolean(installReason) || !installedApp?.installSupported} onclick={() => askToConfirm('update')}>Update</button>
      <button class="danger-button" type="button" disabled={busy || !installedApp?.installSupported} onclick={() => askToConfirm('uninstall')}>Uninstall</button>
    {/if}
  </section>

  <dialog bind:this={actionDialog} class="catalog-confirm" onclose={closeActionDialog} aria-labelledby="catalog-confirm-title">
    <h2 id="catalog-confirm-title">{pendingAction === 'install' ? 'Install' : pendingAction === 'update' ? 'Update' : 'Uninstall'} {app.name}?</h2>
    <p>{pendingAction === 'uninstall' ? 'Remove this Arcade app from your user account.' : `Continue with the ${pendingAction ?? 'requested'} action for ${app.name}?`}</p>
    <div class="dialog-actions">
      <button type="button" onclick={() => actionDialog.close()}>Cancel</button>
      <button class:destructive={pendingAction === 'uninstall'} class="primary" type="button" onclick={confirmAction}>Confirm</button>
    </div>
  </dialog>
</section>
