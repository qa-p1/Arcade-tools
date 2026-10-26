<script lang="ts">
  import Icon from '../components/Icon.svelte';
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
  const lastChecked = $derived(updates?.lastCheck ? new Date(updates.lastCheck * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) : null);
  let openNotes = $state<Record<string, boolean>>({});
  async function toggleNotes(key: string, markdown: string): Promise<void> {
    if (openNotes[key]) { openNotes = { ...openNotes, [key]: false }; return; }
    openNotes = { ...openNotes, [key]: true };
    if (renderedNotes[key] === undefined && !notesError[key]) await revealNotes(key, markdown);
  }
  const resultTone: Record<string, string> = { success: 'ok', failed: 'danger', rolled_back: 'warn', rollback_failed: 'danger' };

  // Notes are kept per release: a newer release brings its own notes.
  const notesKey = (row: Row) => `${row.app.id}@${row.latest?.latestVersion ?? ''}`;
  async function revealNotes(key: string, markdown: string): Promise<void> {
    loadingNotes = { ...loadingNotes, [key]: true };
    try { renderedNotes = { ...renderedNotes, [key]: await api.renderMarkdown(markdown) }; }
    catch (error) { notesError = { ...notesError, [key]: error instanceof Error ? error.message : String(error) }; }
    finally { loadingNotes = { ...loadingNotes, [key]: false }; }
  }

  const rollbacks = $derived(apps.filter((app) => app.managed && app.rollbackVersion));
  const managedApps = $derived(apps.filter((app) => app.managed));
  const appName = (id: string) => apps.find((app) => app.id === id)?.name ?? (id === 'arcade.tools' ? 'Arcade Tools' : id);
  const resultLabels: Record<string, string> = { success: 'Done', failed: 'Failed', rolled_back: 'Rolled back', rollback_failed: 'Rollback failed' };
  const resultLabel = (result: string) => resultLabels[result] ?? result;
  function versions(entry: HistoryEntry): string | null {
    if (!entry.from && !entry.to) return null;
    if (!entry.to) return `from ${entry.from}`;
    return entry.from ? `${entry.from} → ${entry.to}` : entry.to;
  }
  /** -1, 0 or 1 by dotted numeric parts; good enough to word a button. */
  function compare(a: string, b: string): number {
    const parse = (v: string) => v.split(/[.+-]/).map((part) => Number.parseInt(part, 10)).map((n) => Number.isNaN(n) ? 0 : n);
    const [x, y] = [parse(a), parse(b)];
    for (let i = 0; i < Math.max(x.length, y.length); i++) {
      if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) < (y[i] ?? 0) ? -1 : 1;
    }
    return 0;
  }
  const rollbackLabel = (app: AppView) =>
    app.version && app.rollbackVersion && compare(app.rollbackVersion, app.version) > 0 ? `Switch to v${app.rollbackVersion}` : `Roll back to v${app.rollbackVersion}`;

  async function loadHistory(): Promise<void> {
    history = (await api.updateHistory()).slice(-20).reverse();
  }

  // History reloads whenever the tab opens or an operation finishes.
  $effect(() => { if (active && !busy) void loadHistory().catch(() => {}); });
</script>

<div id="panel-updates" class="tab-panel" role="tabpanel" aria-labelledby="tab-updates" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Updates</h1>
      <p class="sub">{lastChecked ? `Last checked ${lastChecked}` : 'Not checked yet'}{updateChecksEnabled ? '' : ' · automatic checks paused'}</p>
    </div>
    <div class="page-actions">
      {#if rows.length > 1}<button type="button" class="btn primary" disabled={busy || checking} onclick={() => run(() => onUpdateAll(rows.map((row) => row.app)), 'Arcade apps')}>Review all updates</button>{/if}
      <button type="button" class="btn" disabled={busy || checking} onclick={() => run(onCheckNow)}><Icon name="refresh" size={14} />{checking ? 'Checking…' : 'Check now'}</button>
    </div>
  </header>

  {#if updates?.lastFailure}<div class="notice warn"><Icon name="alert" /><span class="row-main">The last check didn't finish: {updates.lastFailure.reason}</span></div>{/if}

  {#if rows.length}
    <div class="list update-list">
      {#each rows as row (row.app.id)}
        {@const notes = row.latest?.cache?.release.notes ?? ''}
        {@const key = notesKey(row)}
        <section class="update-card" aria-labelledby="update-{row.app.id}">
          <div class="row">
            <span class="glyph-tile"><img src="/glyphs/{row.app.id}.svg" alt="" width="18" height="18" /></span>
            <div class="row-main">
              <h2 class="row-title" id="update-{row.app.id}">{row.app.name}</h2>
              <div class="row-sub">{row.state.installedVersion ?? row.app.version ?? '?'} → <strong class="to-version">{row.latest?.latestVersion ?? 'new version'}</strong>{row.state.channel !== 'stable' ? ` · ${row.state.channel}` : ''}</div>
            </div>
            <div class="row-end">
              {#if notes}<button type="button" class="btn ghost sm" aria-expanded={Boolean(openNotes[key])} disabled={loadingNotes[key]} onclick={() => run(() => toggleNotes(key, notes), row.app.name)}>{loadingNotes[key] ? 'Loading…' : openNotes[key] ? 'Hide notes' : 'Show release notes'}</button>{/if}
              <button class="btn sm primary primary-action" type="button" disabled={busy || checking || row.state.heldBack} onclick={() => run(() => onUpdate(row.app), row.app.name)}>{row.state.heldBack ? 'Blocked' : 'Review update'}</button>
            </div>
          </div>
          {#if row.state.heldBack || row.latest?.error}
            <div class="row-extra">
              {#if row.state.heldBack}<p class="status-warning">Held back: {row.state.reason ?? row.latest?.reason ?? 'It is not compatible with a connected app.'}</p>{/if}
              {#if row.latest?.error}<p class="status-warning">Release check issue: {row.latest.error}</p>{/if}
            </div>
          {/if}
          {#if notes && openNotes[key] && (renderedNotes[key] !== undefined || notesError[key])}
            <div class="row-extra release-notes">
              {#if notesError[key]}<p class="status-warning">{notesError[key]}</p><pre class="release-plain">{notes}</pre>{:else}<div class="markdown-body">{@html renderedNotes[key]}</div>{/if}
            </div>
          {/if}
        </section>
      {/each}
    </div>
  {:else}
    <div class="empty-state">
      <Icon name={updates?.lastFailure ? 'alert' : updates?.lastCheck ? 'check' : 'updates'} size={24} />
      <h2>{updates?.lastFailure ? 'Could not finish the last check' : updates?.lastCheck ? 'You are up to date' : 'Not checked yet'}</h2>
      <p>{updates?.lastFailure ? 'Check your connection and try again.' : updates?.lastCheck ? 'New versions of your apps will show up here.' : 'Check now to see whether your apps have updates.'}</p>
      <div class="btn-group">
        <button type="button" class="btn primary" disabled={busy || checking} onclick={() => run(onCheckNow)}>{checking ? 'Checking…' : 'Check now'}</button>
        <button type="button" class="btn ghost" onclick={onSelectApps}>Browse apps</button>
      </div>
    </div>
  {/if}

  {#if managedApps.length}
    <section class="section" aria-labelledby="auto-update-heading">
      <div class="section-head"><h2 id="auto-update-heading">Install updates automatically</h2></div>
      <div class="list">
        {#each managedApps as app (app.id)}
          <label class="row compact">
            <span class="glyph-tile sm"><img src="/glyphs/{app.id}.svg" alt="" width="15" height="15" /></span>
            <span class="row-main">{app.name}</span>
            <input class="switch" type="checkbox" aria-label="Update {app.name} automatically" checked={autoUpdate[app.id] ?? false} disabled={busy} onchange={(event) => run(() => onAutoUpdate(app, event.currentTarget.checked), app.name)} />
          </label>
        {/each}
      </div>
      <p class="section-note">A running app updates only when it's idle; if it reports when it was last used, Tools waits ten minutes after that.</p>
    </section>
  {/if}

  {#if rollbacks.length}
    <section class="section" aria-labelledby="rollback-heading">
      <div class="section-head"><h2 id="rollback-heading">Previous versions</h2><span class="meta">Go back if an update broke something</span></div>
      <div class="list">
        {#each rollbacks as app (app.id)}
          <div class="row compact">
            <span class="glyph-tile sm"><img src="/glyphs/{app.id}.svg" alt="" width="15" height="15" /></span>
            <div class="row-main"><div class="row-title">{app.name}</div><div class="row-sub">v{app.version ?? '?'} now · v{app.rollbackVersion} kept</div></div>
            <button type="button" class="btn sm" disabled={busy || checking} onclick={() => run(() => onRollback(app), app.name)}>{rollbackLabel(app)}</button>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if history.length}
    <section class="section" aria-labelledby="history-heading">
      <div class="section-head"><h2 id="history-heading">Recent activity</h2></div>
      <ul class="list update-history">
        {#each history as entry, i (`${entry.time}:${entry.app}:${i}`)}
          <li class="row compact">
            <div class="row-main"><div class="row-title">{appName(entry.app)} <span class="faint history-versions">{versions(entry) ?? ''}</span></div>{#if entry.reason}<div class="row-sub">{entry.reason}</div>{/if}</div>
            <span class="faint history-time">{new Date(entry.time * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })}</span>
            <span class="badge {resultTone[entry.result] ?? ''}">{resultLabel(entry.result)}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .notice + .list, .notice + .empty-state { margin-top: 12px; }
  .update-card + .update-card { border-top: 1px solid var(--line); }
  h2.row-title { font-size: 13px; font-weight: 600; }
  .to-version { color: var(--text); font-weight: 600; }
  .release-notes { max-height: 340px; overflow: auto; }
  .release-notes .markdown-body { color: var(--muted); }
  .row.compact { min-height: 46px; padding: 8px 14px; }
  .update-history { margin: 0; padding: 0; list-style: none; }
  .history-versions { font-weight: 400; font-size: 12px; }
  .history-time { font-size: 12px; white-space: nowrap; }
</style>
