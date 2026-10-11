<script lang="ts">
  import { api, type DocsSearchHit, type LearnApp } from '../lib/api';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void };
  let { active, run }: Props = $props();
  let apps = $state<LearnApp[]>([]);
  let hits = $state<DocsSearchHit[]>([]);
  let query = $state('');
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let fetchBusy = $state<Record<string, boolean>>({});
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  async function loadApps(): Promise<void> {
    loading = true;
    error = null;
    try { apps = await api.listLearnApps(); loaded = true; }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  async function searchDocs(): Promise<void> {
    const term = query.trim();
    if (term.length < 2) { hits = []; return; }
    loading = true;
    error = null;
    try { hits = await api.searchDocs(term); }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  function queueSearch(): void {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { void searchDocs(); }, 220);
  }

  async function fetchInstalledDocs(app: LearnApp): Promise<void> {
    if (!app.installedVersion) return;
    fetchBusy = { ...fetchBusy, [app.id]: true };
    try {
      await api.fetchDocs(app.id, app.installedVersion);
      await loadApps();
      await searchDocs();
    } finally { fetchBusy = { ...fetchBusy, [app.id]: false }; }
  }

  $effect(() => {
    if (active && !loaded && !loading) void loadApps();
  });
</script>

<div id="panel-learn" class="tab-panel" role="tabpanel" aria-labelledby="tab-learn" tabindex="0" hidden={!active}>
  <header class="page-heading"><div><p class="eyebrow">ARCADE TOOLS / LEARN</p><h1>Find the answer in the docs.</h1><p class="intro">Search the documentation available on this device, or open an app’s guide online.</p></div></header>

  <section class="feature-section" aria-labelledby="learn-search-heading">
    <div class="section-heading"><div><h2 id="learn-search-heading">Search documentation</h2><p class="setting-description">Search cached and bundled app docs. Download an installed version’s docs to make them available offline.</p></div></div>
    <form class="docs-search" onsubmit={(event) => { event.preventDefault(); void searchDocs(); }}>
      <label class="visually-hidden" for="docs-query">Search docs</label>
      <input id="docs-query" type="search" bind:value={query} placeholder="Try “keyboard shortcut” or “export”" oninput={queueSearch} />
      <button type="submit" disabled={loading || query.trim().length < 2}>{loading ? 'Searching…' : 'Search'}</button>
    </form>
    {#if error}<p class="status-warning">{error}</p>{/if}
    {#if query.trim().length >= 2 && !loading && hits.length === 0}<p class="setting-description">No matching pages were found in the available documentation.</p>{/if}
    {#if hits.length}
      <div class="docs-results" aria-label="Documentation search results">
        {#each hits as hit (`${hit.app}:${hit.version}:${hit.path}`)}
          <article class="docs-result"><div class="card-heading"><h3>{hit.title}</h3><span class="badge">{hit.app}</span><span class="badge">v{hit.version}</span></div><p class="pitch">{hit.path}</p><p class="docs-snippet">{hit.snippet}</p></article>
        {/each}
      </div>
    {/if}
  </section>

  <section class="feature-section" aria-labelledby="learn-apps-heading">
    <div class="section-heading"><div><h2 id="learn-apps-heading">App guides</h2><p class="setting-description">Documentation links come from Arcade’s app catalog.</p></div><button type="button" disabled={loading} onclick={() => run(loadApps)}>{loading ? 'Loading…' : 'Refresh list'}</button></div>
    {#if apps.length}
      <div class="learn-app-list">
        {#each apps as app (app.id)}
          <div class="learn-app-row">
            <div class="learn-app-info"><strong>{app.name}</strong><span>{app.installedVersion ? `Installed v${app.installedVersion}` : 'Not installed'} · {app.available ? `${app.source} docs${app.docsVersion ? ` · v${app.docsVersion}` : ''}` : 'No local docs yet'}</span></div>
            <div class="actions-inline">
              {#if app.installedVersion && app.needsFetch}<button type="button" disabled={fetchBusy[app.id]} onclick={() => run(() => fetchInstalledDocs(app), app.name)}>{fetchBusy[app.id] ? 'Downloading…' : 'Download installed-version docs'}</button>{/if}
              <button type="button" class="primary-action" onclick={() => run(() => api.openDocs(app.id), app.name)}>Open guide online</button>
            </div>
          </div>
        {/each}
      </div>
    {:else if !loading}<p class="setting-description">No app guides are available yet.</p>{/if}
  </section>
</div>
