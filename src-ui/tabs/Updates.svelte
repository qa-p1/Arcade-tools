<script lang="ts">
  import { api, type AppView, type AppUpdate, type ChannelUpdate, type UpdateState } from '../lib/api';

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
    onAutoUpdate: (app: AppView, enabled: boolean) => Promise<void>;
    onSelectApps: () => void;
    run: (action: () => Promise<void>, label?: string) => void;
  };
  let { active, apps, updates, autoUpdate, busy, checking, updateChecksEnabled, onCheckNow, onUpdate, onAutoUpdate, onSelectApps, run }: Props = $props();
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

  function updateAll(): Promise<void> {
    return (async () => { for (const row of rows) await onUpdate(row.app); })();
  }
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
      {#if rows.length > 1}<div class="update-all-row"><span>{rows.length} managed apps have updates.</span><button type="button" class="primary-action" disabled={busy || checking} onclick={() => run(updateAll, 'Arcade apps')}>Review all updates</button></div>{/if}
      {#each rows as row (row.app.id)}
        {@const notes = row.latest?.cache?.release.notes ?? ''}
        <section class="feature-card update-card" aria-labelledby="update-{row.app.id}">
          <div class="update-app-heading">
            <img class="app-glyph" src="/glyphs/{row.app.id}.svg" alt="" width="30" height="30" />
            <div><h2 id="update-{row.app.id}">{row.app.name}</h2><p class="setting-description">{row.state.installedVersion ?? row.app.version ?? 'Installed version unknown'} → {row.latest?.latestVersion ?? 'New version'} · {row.state.channel}</p></div>
            <button class="primary-action" type="button" disabled={busy || checking} onclick={() => run(() => onUpdate(row.app), row.app.name)}>Review update</button>
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
          <label class="auto-update-toggle"><input type="checkbox" checked={autoUpdate[row.app.id] ?? false} disabled={busy} onchange={(event) => run(() => onAutoUpdate(row.app, event.currentTarget.checked), row.app.name)} /> Automatically update this app when it is idle</label>
        </section>
      {/each}
    </div>
  {:else}
    <div class="empty-state"><h2>{updates?.lastFailure ? 'Could not finish the last check' : 'You are up to date'}</h2><p>{updates?.lastFailure ? 'Check your connection and try again.' : 'Updates for managed, installed apps will appear here. You can check again at any time.'}</p><button type="button" disabled={busy || checking} class="primary-action" onclick={() => run(onCheckNow)}>{checking ? 'Checking…' : 'Check now'}</button><button type="button" onclick={onSelectApps}>Browse apps</button></div>
  {/if}
</div>
