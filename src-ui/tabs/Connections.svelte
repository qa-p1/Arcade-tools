<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import { api, type ConnectionPeer, type ConnectionsView } from '../lib/api';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void; onSettings: () => void; onLearn: (id: string) => Promise<void> };
  let { active, run, onSettings, onLearn }: Props = $props();
  let connections = $state<ConnectionsView | null>(null);
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let expanded = $state<string | null>(null);
  const peers = $derived(connections?.peers ?? []);

  /** Apps that can take something this app produces, with the action they offer. */
  function sendsTo(source: ConnectionPeer): { target: ConnectionPeer; action: ConnectionPeer['actions'][number] }[] {
    const produced = source.actions.flatMap((action) => action.produces);
    return peers.filter((target) => target.id !== source.id).flatMap((target) => target.actions
      .filter((action) => action.accepts.some((accepted) => produced.some((type) => accepted === type || (accepted === 'file/any' && type.startsWith('file/')))))
      .map((action) => ({ target, action })));
  }
  function preferences(peer: ConnectionPeer): { name: string; allowed: boolean }[] {
    const settings = peer.connectedApps.connectedApps ?? peer.connectedApps.connections ?? peer.connectedApps;
    if (!settings || typeof settings !== 'object') return [];
    return Object.entries(settings).filter(([key, value]) => key !== 'linkEnabled' && typeof value === 'boolean')
      .map(([key, value]) => ({ name: peers.find((app) => app.id === key)?.name ?? key, allowed: value as boolean }));
  }
  const reachable = $derived(peers.filter((peer) => peer.reachable).length);

  async function refresh(): Promise<void> {
    loading = true;
    error = null;
    try { connections = await api.listConnections(); loaded = true; }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  $effect(() => { if (active && !loaded && !loading && !error) void refresh(); });
</script>

<div id="panel-connections" class="tab-panel" role="tabpanel" aria-labelledby="tab-connections" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Connections</h1>
      <p class="sub">{connections ? `${peers.length} ${peers.length === 1 ? 'app' : 'apps'} connected · ${reachable} responding` : 'Arcade apps can pass files and text to each other.'}</p>
    </div>
    <div class="page-actions">
      <button type="button" class="btn ghost" onclick={() => run(() => onLearn('arcade.ecosystem'))}><Icon name="book" size={14} />How it works</button>
      <button type="button" class="btn" disabled={loading} onclick={() => run(refresh)}><Icon name="refresh" size={14} />{loading ? 'Checking…' : 'Refresh'}</button>
    </div>
  </header>

  {#if connections && !connections.linkEnabled}
    <div class="notice warn"><Icon name="alert" /><span class="row-main">Connecting is turned off in Tools. Apps keep working on their own.</span><button type="button" class="btn sm" onclick={onSettings}>Settings</button></div>
  {/if}

  {#if connections}
    {#if peers.length}
      <div class="list peer-list">
        {#each peers as peer (peer.id)}
          {@const targets = sendsTo(peer)}
          {@const allowed = preferences(peer)}
          {@const open = expanded === peer.id}
          <section class="peer" aria-label={peer.name}>
            <button type="button" class="row peer-row" aria-expanded={open} onclick={() => expanded = open ? null : peer.id}>
              <span class="glyph-tile"><img src="/glyphs/{peer.id}.svg" alt="" width="18" height="18" /></span>
              <span class="row-main">
                <span class="row-title">{peer.name}</span>
                <span class="row-sub">{peer.actions.length ? `Offers ${peer.actions.length} ${peer.actions.length === 1 ? 'action' : 'actions'}` : 'Offers no actions'}{targets.length ? ` · sends to ${[...new Set(targets.map((flow) => flow.target.name.replace(/^Arcade /, '')))].join(', ')}` : ''}</span>
              </span>
              {#if !peer.linkEnabled}<span class="badge">Connecting off</span>{/if}
              <span class="badge" class:ok={peer.reachable}><span class="dot" class:ok={peer.reachable}></span>{peer.reachable ? (peer.busy ? 'Busy' : 'Responding') : 'Not running'}</span>
              <span class="chevron" class:open><Icon name="chevronDown" size={15} /></span>
            </button>
            {#if open}
              <div class="peer-detail">
                <div class="detail-col">
                  <h3>What it does for other apps</h3>
                  {#if peer.actions.length}
                    <ul>{#each peer.actions as action (action.id)}<li><span>{action.title}</span>{#if !action.available}<span class="faint"> — {action.reason ?? 'unavailable'}</span>{/if}</li>{/each}</ul>
                  {:else}<p class="faint">Nothing yet.</p>{/if}
                </div>
                <div class="detail-col">
                  <h3>Can send to</h3>
                  {#if targets.length}
                    <ul>{#each targets as flow (`${flow.target.id}:${flow.action.id}`)}<li>{flow.target.name} <span class="faint">· {flow.action.title}</span></li>{/each}</ul>
                  {:else}<p class="faint">No other app takes what it makes.</p>{/if}
                </div>
                {#if allowed.length}
                  <div class="detail-col">
                    <h3>Allowed in its settings</h3>
                    <ul>{#each allowed as entry (entry.name)}<li>{entry.name} <span class="faint">· {entry.allowed ? 'allowed' : 'off'}</span></li>{/each}</ul>
                  </div>
                {/if}
                <div class="detail-foot">
                  <span class="faint">Version {peer.version}{peer.protocols.length ? ` · Link protocol ${peer.protocols.join(', ')}` : ''}</span>
                  <button type="button" class="btn sm" onclick={() => run(() => api.openAppSettings(peer.id), peer.name)}>Open {peer.name} settings</button>
                </div>
              </div>
            {/if}
          </section>
        {/each}
      </div>
      <p class="section-note">Each app decides which others it works with, in its own Connected apps settings.</p>
    {:else}
      <div class="empty-state">
        <Icon name="link" size={24} />
        <h2>No apps to connect yet</h2>
        <p>Arcade apps show up here once they're installed and have run once.</p>
      </div>
    {/if}
  {:else if error}
    <div class="empty-state">
      <Icon name="alert" size={24} />
      <h2>Connections could not be checked</h2>
      <p>{error}</p>
      <div class="btn-group"><button type="button" class="btn" onclick={() => run(refresh)}>Try again</button></div>
    </div>
  {:else}
    <div class="list">{#each [0, 1, 2] as item (item)}<div class="skeleton-row"><span class="glyph-tile skeleton"></span><div class="row-main"><div class="skeleton line skel"></div></div></div>{/each}</div>
  {/if}
</div>

<style>
  .notice { margin-bottom: 12px; }
  .peer + .peer { border-top: 1px solid var(--line); }
  .peer-row { width: 100%; border: 0; background: transparent; color: var(--text); text-align: left; cursor: pointer; }
  .peer-row:hover { background: var(--sunken); }
  .peer-row .row-title, .peer-row .row-sub { display: block; }
  .chevron { display: grid; color: var(--faint); transition: transform .15s; }
  .chevron.open { transform: rotate(180deg); }
  .peer-detail { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px 24px; padding: 4px 14px 14px 62px; }
  h3 { margin-bottom: 6px; color: var(--muted); font-size: 12px; font-weight: 600; }
  ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; font-size: 12.5px; }
  .detail-foot { display: flex; grid-column: 1 / -1; align-items: center; justify-content: space-between; gap: 12px; padding-top: 12px; border-top: 1px solid var(--line); font-size: 12px; }
  .skel { width: 40%; }
</style>
