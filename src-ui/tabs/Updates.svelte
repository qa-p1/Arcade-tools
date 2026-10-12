<script lang="ts">
  import { api, type AppView, type AppUpdate, type ChannelUpdate, type HistoryEntry, type UpdateState } from '../lib/api';

  type Row = { app: AppView; state: AppUpdate; latest: ChannelUpdate | undefined };
  type Props = {
    active: boolean;
    apps: AppView[];
    updates: UpdateState | null;
    autoUpdate: Record<string, boolean>;
    busy: boolean;
    checking: boolean;
    updateChecksEnabled: boolean;
    onCheckNow: () => Promise<void>;
    onUpdate: (app: AppView) => Promise<void>;
    onUpdateAll: (apps: AppView[]) => Promise<void>;
    onRollback: (app: AppView) => Promise<void>;
    onAutoUpdate: (app: AppView, enabled: boolean) => Promise<void>;
    onSelectApps: () => void;
    run: (action: () => Promise<void>, label?: string) => void;
  };
  let { active, apps, updates, autoUpdate, busy, checking, updateChecksEnabled, onCheckNow, onUpdate, onUpdateAll, onRollback, onAutoUpdate, onSelectApps, run }: Props = $props();
  let history = $state<HistoryEntry[]>([]);
  let renderedNotes = $state<Record<string, string>>({});
  let notesError = $state<Record<string, string>>({});
  let loadingNotes = $state<Record<string, boolean>>({});
  const rows = $derived(apps.flatMap((app): Row[] => {
    const state = updates?.apps[app.id];
    if (!app.managed || !state?.updateAvailable) return [];
    return [{ app, state, latest: state.latest[state.channel] }];
  }));
  const lastChecked = $derived(updates?.lastCheck ? new Date(updates.lastCheck * 1000).toLocaleString() : 'Not checked yet');

  async function revealNotes(appId: string, markdown: string): Promise<void> {
    loadingNotes = { ...loadingNotes, [appId]: true };
    try { renderedNotes = { ...renderedNotes, [appId]: await api.renderMarkdown(markdown) }; }
    catch (error) { notesError = { ...notesError, [appId]: error instanceof Error ? error.message : String(error) }; }
    finally { loadingNotes = { ...loadingNotes, [appId]: false }; }
  }

  const rollbacks = $derived(apps.filter((app) => app.managed && app.rollbackVersion));
  const managedApps = $derived(apps.filter((app) => app.managed));
  const appName = (id: string) => apps.find((app) => app.id === id)?.name ?? id;

  async function loadHistory(): Promise<void> {
    history = (await api.updateHistory()).slice(-20).reverse();
  }

  // History reloads whenever the tab opens or an operation finishes.
  $effect(() => { if (active && !busy) void loadHistory().catch(() => {}); });
</script>

<div id="panel-updates" class="tab-panel" role="tabpanel" aria-labelledby="tab-updates" tabindex="0" hidden={!active}>
  <header class="page-heading">
    <div><p class="eyebrow">ARCADE TOOLS / UPDATES</p><h1>Keep your apps current.</h1><p class="intro">Review release notes and confirm each update before it is installed.</p></div>
    <div class="updates-heading-actions"><span class="app-count">{rows.length} available</span><button type="button" disabled={busy || checking} onclick={() => run(onCheckNow)}>{checking ? 'Checking…' : 'Check now'}</button></div>
  </header>

  <section class="feature-section update-status" aria-label="Update check status">
    <div><strong>Last check</strong><span>{lastChecked}</span></div>
    <div><strong>Scheduled checks</strong><span>{updateChecksEnabled ? 'On' : 'Paused'}</span></div>
    {#if updates?.lastFailure}<p class="status-warning">Last check issue: {updates.lastFailure.reason}</p>{/if}
  </section>

  {#if rows.length}
    <div class="update-list">
      {#if rows.length > 1}<div class="update-all-row"><span>{rows.length} managed apps have updates.</span><button type="button" class="primary-action" disabled={busy || checking} onclick={() => run(() => onUpdateAll(rows.map((row) => row.app)), 'Arcade apps')}>Review all updates</button></div>{/if}
      {#each rows as row (row.app.id)}
        {@const notes = row.latest?.cache?.release.notes ?? ''}
        <section class="feature-card update-card" aria-labelledby="update-{row.app.id}">
          <div class="update-app-heading">
            <img class="app-glyph" src="/glyphs/{row.app.id}.svg" alt="" width="30" height="30" />
            <div><h2 id="update-{row.app.id}">{row.app.name}</h2><p class="setting-description">{row.state.installedVersion ?? row.app.version ?? 'Installed version unknown'} → {row.latest?.latestVersion ?? 'New version'} · {row.state.channel}</p></div>
            <button class="primary-action" type="button" disabled={busy || checking || row.state.heldBack} onclick={() => run(() => onUpdate(row.app), row.app.name)}>{row.state.heldBack ? 'Blocked' : 'Review update'}</button>
          </div>
          {#if row.state.heldBack}<p class="status-warning">This update is held back: {row.state.reason ?? row.latest?.reason ?? 'It is not compatible with a connected app.'}</p>{/if}
          {#if row.latest?.error}<p class="status-warning">Release check issue: {row.latest.error}</p>{/if}
          {#if notes}
            <div class="release-notes">
              {#if renderedNotes[row.app.id] !== undefined}
                <details open><summary>Release notes</summary>{#if notesError[row.app.id]}<p class="status-warning">{notesError[row.app.id]}</p>{:else}<div class="markdown-body">{@html renderedNotes[row.app.id]}</div>{/if}</details>
              {:else}
                <button type="button" class="text-action" disabled={loadingNotes[row.app.id]} onclick={() => run(() => revealNotes(row.app.id, notes), row.app.name)}>{loadingNotes[row.app.id] ? 'Rendering notes…' : 'Show release notes'}</button>
              {/if}
            </div>
          {:else}<p class="setting-description">No release notes were provided for this version.</p>{/if}
        </section>
      {/each}
    </div>
  {:else}
    <div class="empty-state"><h2>{updates?.lastFailure ? 'Could not finish the last check' : 'You are up to date'}</h2><p>{updates?.lastFailure ? 'Check your connection and try again.' : 'Updates for managed, installed apps will appear here. You can check again at any time.'}</p><button type="button" disabled={busy || checking} class="primary-action" onclick={() => run(onCheckNow)}>{checking ? 'Checking…' : 'Check now'}</button><button type="button" onclick={onSelectApps}>Browse apps</button></div>
  {/if}
  {#if managedApps.length}
    <section class="feature-section" aria-labelledby="auto-update-heading">
      <h2 id="auto-update-heading">Automatic updates</h2>
      <p class="setting-description">Opt in for each app. Tools waits until it is idle and has not been used for ten minutes.</p>
      {#each managedApps as app (app.id)}<label class="auto-update-toggle"><input type="checkbox" checked={autoUpdate[app.id] ?? false} disabled={busy} onchange={(event) => run(() => onAutoUpdate(app, event.currentTarget.checked), app.name)} /> {app.name}</label>{/each}
    </section>
  {/if}
  {#if rollbacks.length}
    <section class="feature-section" aria-labelledby="rollback-heading">
      <div class="section-heading"><div><h2 id="rollback-heading">Previous versions</h2><p class="setting-description">Tools keeps the build an update replaced, so you can go back if something broke.</p></div></div>
      <div class="home-app-list">
        {#each rollbacks as app (app.id)}
          <div class="home-app-row"><img class="app-glyph" src="/glyphs/{app.id}.svg" alt="" width="30" height="30" /><div class="home-app-info"><strong>{app.name}</strong><span>v{app.version ?? '?'} now · v{app.rollbackVersion} saved</span></div><button type="button" disabled={busy || checking} onclick={() => run(() => onRollback(app), app.name)}>Roll back</button></div>
        {/each}
      </div>
    </section>
  {/if}

  {#if history.length}
    <section class="feature-section" aria-labelledby="history-heading">
      <div class="section-heading"><div><h2 id="history-heading">Recent activity</h2><p class="setting-description">Installs, updates and rollbacks Tools ran on this device.</p></div></div>
      <ul class="update-history">
        {#each history as entry, i (`${entry.time}:${entry.app}:${i}`)}
          <li><span>{new Date(entry.time * 1000).toLocaleString()}</span><strong>{appName(entry.app)}</strong><span>{entry.from ?? '—'} → {entry.to ?? '—'}</span><span class="badge">{entry.result}</span>{#if entry.reason}<span class="setting-description">{entry.reason}</span>{/if}</li>
        {/each}
      </ul>
    </section>
  {/if}
</div>
