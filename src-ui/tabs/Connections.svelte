<script lang="ts">
  import { api, type ConnectionsView } from '../lib/api';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void; onSettings: () => void };
  let { active, run, onSettings }: Props = $props();
  let connections = $state<ConnectionsView | null>(null);
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);

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
    <section class="feature-section connection-summary"><div><strong>Arcade Link in Tools</strong><span class:status-on={connections.linkEnabled} class:status-off={!connections.linkEnabled}>{connections.linkEnabled ? 'Enabled' : 'Disabled'}</span></div><button type="button" onclick={onSettings}>Open Settings</button></section>
    {#if connections.peers.length}
      <div class="connection-list">
        {#each connections.peers as peer (peer.id)}
          <section class="feature-card connection-card">
            <div class="connection-heading"><div><h2>{peer.name}</h2><p class="setting-description">{peer.id} · v{peer.version}</p></div><span class="badge" class:reachable={peer.reachable} class:unreachable={!peer.reachable}>{peer.reachable ? 'Reachable' : 'Not responding'}</span></div>
            <div class="connection-facts"><span>Link {peer.linkEnabled ? 'enabled' : 'disabled'} in this app</span><span>Protocol {peer.protocols.join(', ') || 'unknown'}</span>{#if peer.busy !== null}<span>{peer.busy ? 'Busy' : 'Idle'}</span>{/if}</div>
            {#if peer.actions.length}<div class="connection-actions"><h3>Actions this app offers</h3><ul>{#each peer.actions as action (action.id)}<li><span>{action.title}</span><span class="badge">{action.available ? action.verb : action.reason ?? 'Unavailable'}</span></li>{/each}</ul></div>{:else}<p class="setting-description">This app has not advertised any actions to Tools.</p>{/if}
          </section>
        {/each}
      </div>
    {:else}<div class="empty-state"><h2>No other Arcade apps found</h2><p>Installed apps appear here when they register with Arcade Link and have a local manifest.</p></div>{/if}
  {:else if error}<div class="empty-state"><h2>Connections could not be checked</h2><p>{error}</p><button type="button" onclick={() => run(refresh)}>Try again</button></div>{:else}<div class="empty-state"><p>{loading ? 'Checking local Arcade Link peers…' : 'Connection details have not been loaded.'}</p></div>{/if}
</div>
