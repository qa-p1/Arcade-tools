<script lang="ts">
  import { confirmOperation, changeChannel, changeLogin, launchApp } from '../lib/controller';
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import AppShortcutRow from './AppShortcutRow.svelte';
  import KeyChips from './KeyChips.svelte';
  import Icon from './Icon.svelte';
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
  async function loadHistory(): Promise<void> {
    const id = app.id;
    const found = await api.releaseHistory(id);
    // The same component shows the next app if the user moves on.
    if (app.id === id) history = found;
  }
  let ownIcon = $state(false);
  $effect(() => { const id=app.id; void invoke<{ preferences: { excluded: string[] } }>('tray_get_settings').then((view) => ownIcon=view.preferences.excluded.includes(id)).catch(() => {}); });
  async function setOwnIcon(enabled: boolean): Promise<void> {
    const view = await invoke<{ preferences: { excluded: string[] } }>('tray_get_settings');
    const excluded = view.preferences.excluded.filter((id) => id !== app.id);
    if (enabled) excluded.push(app.id);
    await invoke('tray_set_settings', { preferences: { ...view.preferences, excluded } });
    ownIcon = enabled;
  }
  // Notes checked from an app card arrive without rendered HTML: render them
  // here, and fall back to the plain text if that fails.
  let renderedNotes = $state<{ key: string; html: string | null; error: string | null } | null>(null);
  $effect(() => {
    const notes = latest?.notes;
    const key = `${app.id}@${latest?.version ?? ''}`;
    if (!notes || releaseNotesHtml || renderedNotes?.key === key) return;
    renderedNotes = { key, html: null, error: null };
    api.renderMarkdown(notes)
      .then((html) => { if (renderedNotes?.key === key) renderedNotes = { key, html, error: null }; })
      .catch((error) => { if (renderedNotes?.key === key) renderedNotes = { key, html: null, error: error?.message ?? String(error) }; });
  });
  const notesHtml = $derived(releaseNotesHtml ?? (renderedNotes?.key === `${app.id}@${latest?.version ?? ''}` ? renderedNotes.html : null));
  const notesError = $derived(!releaseNotesHtml && renderedNotes?.key === `${app.id}@${latest?.version ?? ''}` ? renderedNotes.error : null);
  let brokenShots = $state<string[]>([]);
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

  const platformNames: Record<string, string> = { linux: 'Linux', windows: 'Windows', macos: 'macOS' };
  const platformStatus: Record<string, string> = { verified: 'Tested', ci: 'CI builds only', unsupported: 'Not available' };

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
  <button class="btn ghost sm back-button" type="button" onclick={onBack}>← Back to apps</button>
  <header class="detail-head">
    <span class="glyph-tile lg"><img src="/glyphs/{app.id}.svg" alt="" width="26" height="26" /></span>
    <div class="row-main">
      <h1 id="catalog-detail-title">{app.name}</h1>
      <p class="sub">{app.pitch}</p>
    </div>
    <div class="detail-actions" aria-label="Catalog app actions">
      {#if installedApp && installed}<button class="btn" type="button" disabled={busy} onclick={() => run(() => launchApp(installedApp!, 'foreground'), app.name)}>Open</button>{/if}
      {#if !installed}
        <button class="btn primary" type="button" disabled={busy || Boolean(installReason) || !installedApp?.installSupported} onclick={() => askToConfirm('install')}>Install</button>
      {:else}
        <button class="btn primary" type="button" disabled={busy || Boolean(installReason) || !installedApp?.installSupported} onclick={() => askToConfirm('update')}>Update</button>
      {/if}
    </div>
  </header>

  <div class="detail-grid">
    <div class="detail-main">
      <section class="detail-block" aria-labelledby="detail-about-title">
        <h2 id="detail-about-title">About</h2>
        <p class="about">{app.description}</p>
        {#if installReason}<p class="notice warn about-note"><Icon name="alert" />{installReason}</p>{/if}
      </section>

      {#if screenshots.length}
        <section class="detail-block" aria-labelledby="detail-screenshots-title">
          <h2 id="detail-screenshots-title">Screenshots</h2>
          <div class="screenshot-grid">
            {#each screenshots as source, index (`${source}-${index}`)}
              {#if brokenShots.includes(source)}
                <div class="screenshot-missing" role="img" aria-label="{app.name} screenshot {index + 1} unavailable">Screenshot unavailable</div>
              {:else}
                <img src={source} alt="{app.name} screenshot {index + 1}" loading="lazy" onerror={() => brokenShots = [...brokenShots, source]} />
              {/if}
            {/each}
          </div>
        </section>
      {/if}

      <section class="detail-block" aria-labelledby="detail-release-notes-title">
        <div class="block-head">
          <h2 id="detail-release-notes-title">{latest?.notes ? `What's new in ${latest.version}` : 'Releases'}</h2>
          <button class="btn sm" type="button" disabled={busy} onclick={() => run(onCheckLatest, app.name)}>Check latest</button>
        </div>
        {#if latest?.notes}
          <div class="card release-notes">
            {#if notesHtml}<div class="markdown-body">{@html notesHtml}</div>
            {:else if notesError}<p class="status-warning">The notes couldn't be formatted ({notesError}).</p><pre class="release-plain">{latest.notes}</pre>
            {:else}<p class="muted">Preparing release notes…</p>{/if}
          </div>
        {:else}
          <p class="muted">Check the latest release to read what changed.</p>
        {/if}
        <section class="release-history" aria-label="Release history">
          {#if history.length}
            <div class="list">
              {#each history as release (`${release.version}-${release.publishedAt}`)}
                <details class="history-item"><summary><span class="row-main">{release.version}</span><span class="faint">{release.publishedAt}{release.prerelease ? ' · preview' : ''}</span><Icon name="chevronDown" size={14} /></summary><div class="markdown-body">{@html release.html}</div></details>
              {/each}
            </div>
          {:else}
            <button class="link-button" type="button" disabled={busy} onclick={() => run(loadHistory, app.name)}>Load version history</button>
          {/if}
        </section>
      </section>

      <section class="detail-block" aria-labelledby="detail-shortcuts-title">
        <h2 id="detail-shortcuts-title">Shortcuts</h2>
        {#if shortcutSheet || arcadeShortcuts.length}
          {#if arcadeShortcuts.length}
            <ul class="list plain-list">
              {#each arcadeShortcuts as shortcut (`${shortcut.app}-${shortcut.id}`)}
                <li class="row"><div class="row-main"><div class="row-title">{shortcut.title}</div><div class="row-sub">Works everywhere</div></div><KeyChips keys={shortcut.display} os={shortcuts?.os ?? 'linux'} /></li>
              {/each}
            </ul>
          {/if}
          {#if shortcutSheet}
            {#each shortcutSheet.groups as group (group.title)}
              <h3 class="group-title">{group.title}</h3>
              <ul class="list plain-list">
                {#each group.shortcuts as shortcut (shortcut.id)}<AppShortcutRow {shortcut} os={shortcuts?.os ?? 'linux'} />{/each}
              </ul>
            {/each}
          {/if}
        {:else}
          <p class="muted">No shortcut sheet for this app yet.</p>
        {/if}
      </section>

      <section class="detail-block" aria-labelledby="detail-connections-title">
        <h2 id="detail-connections-title">Works with your other apps</h2>
        {#if offeredActions.length}
          <ul class="list plain-list">
            {#each offeredActions as action (action.id)}
              <li class="row"><div class="row-main"><div class="row-title">{action.title}</div>{#if action.accepts.length}<div class="row-sub">Accepts {action.accepts.join(', ')}</div>{/if}</div>{#if !action.available}<span class="badge">{action.reason ?? 'Unavailable'}</span>{/if}</li>
            {/each}
          </ul>
        {:else}
          <p class="muted">Start this app to see what it offers the others. <button class="link-button" type="button" onclick={() => run(onOpenDocs, app.name)}>Read the guide</button></p>
        {/if}
      </section>
    </div>

    <aside class="detail-side">
      <div class="card side-card">
        <dl class="facts">
          <div><dt>Installed</dt><dd>{installedApp?.version ? `v${installedApp.version}` : 'Not installed'}</dd></div>
          <div><dt>Latest</dt><dd>{latest?.version ? `v${latest.version}` : 'Not checked'}</dd></div>
          <div><dt>Category</dt><dd class="cap">{app.category}</dd></div>
          {#each Object.entries(app.platforms) as [platform, status] (platform)}
            <div><dt>{platformNames[platform] ?? platform}</dt><dd>{platformStatus[status] ?? status}</dd></div>
          {/each}
        </dl>
      </div>

      {#if installedApp && installed}
        <div class="card side-card" aria-label="Installed app management" role="group">
          <h3>Manage</h3>
          <label class="switch-row"><span>Channel</span><select class="select sm" value={installedApp.channel} disabled={busy} onchange={(event) => run(() => changeChannel(installedApp!, event.currentTarget.value as 'stable' | 'nightly'), app.name)}><option value="stable">Stable</option><option value="nightly">Nightly</option></select></label>
          {#if installedApp.loginSupported}<label class="switch-row"><span>Start at login</span><input class="switch" type="checkbox" checked={installedApp.startAtLogin} disabled={busy} onchange={(event) => run(() => changeLogin(installedApp!, event.currentTarget.checked), app.name)} /></label>{/if}
          <label class="switch-row"><span>Keep its own tray icon</span><input class="switch" type="checkbox" checked={ownIcon} disabled={busy} onchange={(event) => run(() => setOwnIcon(event.currentTarget.checked), app.name)} /></label>
          <div class="side-buttons">
            <button class="btn sm" type="button" disabled={busy} onclick={() => run(() => api.openAppSettings(app.id), app.name)}>App settings</button>
            <button class="btn sm" type="button" disabled={busy} onclick={() => run(() => launchApp(installedApp!, 'background'), app.name)}>Open in background</button>
            {#if installedApp.managed}<button class="btn sm" type="button" disabled={busy} onclick={() => run(() => confirmOperation(installedApp!, 'repair'), app.name)}>Repair</button>{/if}
            {#if !installedApp.managed && installedApp.installationClass !== 'dev'}<button class="btn sm" type="button" disabled={busy} onclick={() => run(() => confirmOperation(installedApp!, 'adopt'), app.name)}>Let Tools manage it</button>{/if}
            {#if installedApp.rollbackVersion}<button class="btn sm" type="button" disabled={busy} onclick={() => run(() => confirmOperation(installedApp!, 'rollback'), app.name)}>Roll back to {installedApp.rollbackVersion}</button>{/if}
            <button class="btn sm danger" type="button" disabled={busy || !installedApp?.installSupported} onclick={() => askToConfirm('uninstall')}>Uninstall</button>
          </div>
        </div>
      {/if}

      {#if appEngines.length}
        <div class="card side-card">
          <h3>Uses</h3>
          <ul class="engine-names">
            {#each appEngines as engine (engine.id)}<li><a href={engine.homepage} target="_blank" rel="noreferrer">{engine.name}</a></li>{/each}
          </ul>
        </div>
      {/if}

      <div class="side-links">
        <button class="btn ghost sm" type="button" onclick={() => run(onOpenDocs, app.name)}><Icon name="book" size={14} />Documentation</button>
        <a class="btn ghost sm" href={app.homepage} target="_blank" rel="noreferrer"><Icon name="external" size={14} />Repository</a>
      </div>
    </aside>
  </div>

  <dialog bind:this={actionDialog} class="catalog-confirm" onclose={closeActionDialog} aria-labelledby="catalog-confirm-title">
    <h2 id="catalog-confirm-title">{pendingAction === 'install' ? 'Install' : pendingAction === 'update' ? 'Update' : 'Uninstall'} {app.name}?</h2>
    <p>{pendingAction === 'uninstall' ? 'Remove this Arcade app from your user account.' : `Continue with the ${pendingAction ?? 'requested'} action for ${app.name}?`}</p>
    <div class="dialog-actions">
      <button class="btn" type="button" onclick={() => actionDialog.close()}>Cancel</button>
      <button class="btn" class:destructive={pendingAction === 'uninstall'} class:primary={pendingAction !== 'uninstall'} type="button" onclick={confirmAction}>Confirm</button>
    </div>
  </dialog>
</section>

<style>
  .back-button { margin: -6px 0 14px -8px; }
  .detail-head { display: flex; align-items: center; gap: 16px; margin-bottom: 28px; }
  .detail-head h1 { font-size: 21px; font-weight: 600; letter-spacing: -.015em; }
  .detail-head .sub { margin-top: 2px; color: var(--muted); }
  .detail-actions { display: flex; gap: 8px; }
  .detail-grid { display: grid; grid-template-columns: minmax(0, 1fr) 260px; gap: 28px; align-items: start; }
  .detail-block + .detail-block { margin-top: 30px; }
  .detail-block h2 { margin-bottom: 10px; font-size: 14px; font-weight: 600; }
  .block-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }
  .block-head h2 { margin: 0; }
  .about { color: var(--muted); line-height: 1.65; }
  .about-note { margin-top: 12px; }
  .release-notes { max-height: 360px; overflow: auto; }
  .release-history { margin-top: 12px; }
  .history-item summary { display: flex; align-items: center; gap: 10px; padding: 10px 14px; cursor: pointer; list-style: none; }
  .history-item summary::-webkit-details-marker { display: none; }
  .history-item[open] summary :global(.icon) { transform: rotate(180deg); }
  .history-item + .history-item { border-top: 1px solid var(--line); }
  .history-item .markdown-body { padding: 0 14px 14px; color: var(--muted); }
  .plain-list { margin: 0; padding: 0; list-style: none; }
  .group-title { margin: 16px 0 8px; color: var(--faint); font-size: 11px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; }
  .screenshot-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 10px; }
  .screenshot-grid img { display: block; width: 100%; max-height: 260px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--sunken); object-fit: contain; }
  .screenshot-missing { display: grid; min-height: 120px; place-items: center; border: 1px dashed var(--line-strong); border-radius: var(--radius-sm); color: var(--muted); }
  .detail-side { display: grid; gap: 12px; position: sticky; top: 24px; }
  .side-card { display: grid; gap: 12px; }
  .side-card + .side-card { margin-top: 0; }
  .side-card h3 { font-size: 12px; font-weight: 600; color: var(--muted); }
  .facts { display: grid; gap: 8px; margin: 0; }
  .facts div { display: flex; justify-content: space-between; gap: 12px; font-size: 12.5px; }
  .facts dt { color: var(--muted); }
  .facts dd { margin: 0; font-weight: 500; text-align: right; }
  .cap { text-transform: capitalize; }
  .side-card .switch-row { font-size: 12.5px; }
  .side-buttons { display: flex; flex-wrap: wrap; gap: 6px; padding-top: 4px; }
  .engine-names { display: grid; gap: 6px; margin: 0; padding: 0; list-style: none; font-size: 12.5px; }
  .side-links { display: flex; flex-wrap: wrap; gap: 2px; margin-left: -8px; }
  @media (max-width: 860px) {
    .detail-grid { grid-template-columns: 1fr; }
    .detail-side { position: static; }
    .detail-head { flex-wrap: wrap; }
  }
</style>
