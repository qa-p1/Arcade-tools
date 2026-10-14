<script lang="ts">
  import { api, type ConnectionsView } from '../lib/api';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void; onSettings: () => void; onLearn: (id: string) => Promise<void> };
  let { active, run, onSettings, onLearn }: Props = $props();
  let connections = $state<ConnectionsView | null>(null);
  const flows = $derived((connections?.peers ?? []).flatMap((source) => (connections?.peers ?? []).filter((target) => source.id !== target.id).flatMap((target) => target.actions.filter((action) => action.accepts.some((accepted) => source.actions.some((offered) => offered.produces.some((produced) => accepted === produced || accepted === 'file/any' && produced.startsWith('file/'))))).map((action) => ({ source, target, action })))));
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  function position(id: string) {
    const peers = connections?.peers ?? [];
    const angle = peers.findIndex((peer) => peer.id === id) / Math.max(peers.length, 1) * Math.PI * 2 - Math.PI / 2;
    return { x: 450 + Math.cos(angle) * 290, y: 210 + Math.sin(angle) * 150 };
  }
  function preferences(peer: NonNullable<ConnectionsView>['peers'][number]): string[] {
    const settings = peer.connectedApps.connectedApps ?? peer.connectedApps.connections ?? peer.connectedApps;
    if (!settings || typeof settings !== 'object') return [];
    return Object.entries(settings).filter(([key, value]) => key !== 'linkEnabled' && typeof value === 'boolean').map(([key, value]) => `${connections?.peers.find((app) => app.id === key)?.name ?? key}: ${value ? 'allowed' : 'off'}`);
  }

  async function refresh(): Promise<void> {
    loading = true;
    error = null;
    try { connections = await api.listConnections(); loaded = true; }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  $effect(() => { if (active && !loaded && !loading) void refresh(); });
</script>

<div id="panel-connections" class="tab-panel" role="tabpanel" aria-labelledby="tab-connections" tabindex="0" hidden={!active}>
  <header class="page-heading"><div><p class="eyebrow">ARCADE TOOLS / CONNECTIONS</p><h1>See which apps can talk.</h1><p class="intro">Arcade Link lists the apps that have registered with Tools and checks whether their local endpoint responds.</p></div><button type="button" disabled={loading} onclick={() => run(refresh)}>{loading ? 'Checking…' : 'Refresh connections'}</button></header>

  {#if connections}
    <section class="feature-section connection-summary"><div><strong>Arcade Link in Tools</strong><span class:status-on={connections.linkEnabled} class:status-off={!connections.linkEnabled}>{connections.linkEnabled ? 'Enabled' : 'Disabled'}</span></div><div class="actions-inline"><button type="button" onclick={() => run(() => onLearn('arcade.ecosystem'))}>How apps connect</button><button type="button" onclick={onSettings}>Open Settings</button></div></section>
    <section class="feature-section" aria-label="Integration map">
      <h2>Integrations</h2>
      {#if connections.peers.length}
        <svg class="connection-map" viewBox="0 0 900 420" role="img" aria-labelledby="connection-map-title">
          <title id="connection-map-title">Arcade app integrations; arrows point to the app offering an action</title>
          <defs><marker id="connection-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor" /></marker></defs>
          {#each flows as flow (`${flow.source.id}:${flow.target.id}:${flow.action.id}`)}
            {@const from = position(flow.source.id)}{@const to = position(flow.target.id)}
            <line x1={from.x} y1={from.y} x2={to.x} y2={to.y} stroke="currentColor" marker-end="url(#connection-arrow)" opacity={flow.action.available ? 0.45 : 0.2}><title>{flow.source.name} → {flow.target.name}: {flow.action.title}</title></line>
          {/each}
          {#each connections.peers as peer (peer.id)}{@const point = position(peer.id)}<g><rect x={point.x - 85} y={point.y - 23} width="170" height="46" rx="10" fill="var(--surface)" stroke="currentColor" /><text x={point.x} y={point.y + 5} text-anchor="middle" fill="currentColor">{peer.name}</text></g>{/each}
        </svg>
      {/if}
      <div class="integration-nodes">{#each connections.peers as peer (peer.id)}<button type="button" onclick={() => run(() => api.openAppSettings(peer.id), peer.name)}>{peer.name} · {peer.linkEnabled ? 'Link on' : 'Link off'}</button>{/each}</div>
      <ul>{#each flows as flow (`${flow.source.id}:${flow.target.id}:${flow.action.id}`)}<li>{flow.source.name} → {flow.target.name}: {flow.action.title}</li>{/each}</ul>
      <p class="setting-description">Connections follow the content types advertised in app manifests. Choose an app to change its connected-app preferences there.</p>
      <button type="button" onclick={() => run(() => onLearn('arcade.ecosystem'))}>Learn the Find → Look → Box and Lens → Clipboard flows</button>
    </section>
    {#if connections.peers.length}
      <div class="connection-list">
        {#each connections.peers as peer (peer.id)}
          <section class="feature-card connection-card">
            <div class="connection-heading"><div><h2>{peer.name}</h2><p class="setting-description">{peer.id} · v{peer.version}</p></div><span class="badge" class:reachable={peer.reachable} class:unreachable={!peer.reachable}>{peer.reachable ? 'Reachable' : 'Not responding'}</span></div>
            <button type="button" onclick={() => run(() => api.openAppSettings(peer.id), peer.name)}>Open {peer.name} settings</button>
            <div class="connection-facts"><span>Link {peer.linkEnabled ? 'enabled' : 'disabled'} in this app</span><span>Protocol {peer.protocols.join(', ') || 'unknown'}</span>{#if peer.busy !== null}<span>{peer.busy ? 'Busy' : 'Idle'}</span>{/if}</div>
            {#if preferences(peer).length}<ul aria-label="Connected-app preferences">{#each preferences(peer) as preference (preference)}<li>{preference}</li>{/each}</ul>{/if}
            {#if peer.actions.length}<div class="connection-actions"><h3>Actions this app offers</h3><ul>{#each peer.actions as action (action.id)}<li><span>{action.title}</span><span class="badge">{action.available ? action.verb : action.reason ?? 'Unavailable'}</span></li>{/each}</ul></div>{:else}<p class="setting-description">This app has not advertised any actions to Tools.</p>{/if}
          </section>
        {/each}
      </div>
    {:else}<div class="empty-state"><h2>No other Arcade apps found</h2><p>Installed apps appear here when they register with Arcade Link and have a local manifest.</p></div>{/if}
  {:else if error}<div class="empty-state"><h2>Connections could not be checked</h2><p>{error}</p><button type="button" onclick={() => run(refresh)}>Try again</button></div>{:else}<div class="empty-state"><p>{loading ? 'Checking local Arcade Link peers…' : 'Connection details have not been loaded.'}</p></div>{/if}
</div>

<style>.connection-map { width: 100%; max-height: 28rem; } .connection-map text { font-size: 14px; }</style>
