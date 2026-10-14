<script lang="ts">
  import { api, type DocPage, type DocsSearchHit, type LearnApp } from '../lib/api';
  import { onDestroy } from 'svelte';

  type Props = { active: boolean; target: string; run: (action: () => Promise<void>, label?: string) => void };
  let { active, target, run }: Props = $props();
  let readerApp = $state<string | null>(null);
  let pages = $state<{ path: string; title: string }[]>([]);
  let page = $state<DocPage | null>(null);
  let readerError = $state<string | null>(null);
  let latest = $state(false);
  let openedTarget = '';
  let apps = $state<LearnApp[]>([]);
  let hits = $state<DocsSearchHit[]>([]);
  let query = $state('');
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let fetchBusy = $state<Record<string, boolean>>({});
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => { if (searchTimer) clearTimeout(searchTimer); });

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

  async function openReader(id: string, path?: string): Promise<void> {
    readerError = null;
    try {
      if (readerApp !== id) { latest = false; readerApp = id; }
      pages = await api.docsPages(id, latest);
      const chosen = path ?? pages[0]?.path;
      page = chosen ? await api.readDoc(id, chosen, latest) : null;
      if (!chosen) readerError = 'No pages are available offline for this app yet.';
    } catch (problem) { readerError = problem instanceof Error ? problem.message : String(problem); }
  }

  async function changeVersion(useLatest: boolean): Promise<void> {
    latest = useLatest;
    if (readerApp) await openReader(readerApp);
  }
  async function fetchLatest(): Promise<void> {
    if (!readerApp) return;
    const id = readerApp;
    fetchBusy = { ...fetchBusy, [id]: true };
    try {
      await api.fetchLatestDocs(id);
      latest = true;
      await openReader(id);
      await loadApps();
    } finally { fetchBusy = { ...fetchBusy, [id]: false }; }
  }
  function followLink(event: MouseEvent): void {
    const link = (event.target as Element).closest('a');
    if (!link || !readerApp || !page) return;
    const href = link.getAttribute('href') ?? '';
    if (!href || href.startsWith('#')) return;
    if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('//')) return;
    event.preventDefault();
    const base = new URL(page.path, 'https://arcade-docs.invalid/');
    const target = new URL(href, base);
    const path = decodeURIComponent(target.pathname.slice(1));
    if (pages.some((entry) => entry.path === path)) run(() => openReader(readerApp!, path));
    else readerError = 'This link points outside the available guide. Open the guide online to follow it.';
  }
  function docsNavigation(node: HTMLElement) {
    node.addEventListener('click', followLink);
    return { destroy() { node.removeEventListener('click', followLink); } };
  }

  $effect(() => {
    if (active && !loaded && !loading) void loadApps();
  });
  // Other tabs open Learn on a specific app's guide.
  $effect(() => {
    if (active && target && target !== openedTarget) { openedTarget = target; void openReader(target); }
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
          <article class="docs-result"><div class="card-heading"><h3>{hit.title}</h3><span class="badge">{hit.app}</span><span class="badge">v{hit.version}</span></div><p class="pitch">{hit.path}</p><p class="docs-snippet">{hit.snippet}</p><button type="button" onclick={() => run(() => openReader(hit.app, hit.path))}>Read this page</button></article>
        {/each}
      </div>
    {/if}
  </section>

  {#if readerApp}
    <section class="feature-section docs-reader" aria-labelledby="learn-reader-heading">
      <div class="section-heading"><div><h2 id="learn-reader-heading">{apps.find((app) => app.id === readerApp)?.name ?? (readerApp === 'arcade.ecosystem' ? 'Arcade' : readerApp)} guide</h2>{#if page}<p class="setting-description">v{page.version} · {page.source}</p>{/if}</div><button type="button" onclick={() => { readerApp = null; page = null; }}>Close</button></div>
      {#if readerError}<p class="status-warning">{readerError}</p>{/if}
      {#if readerApp !== 'arcade.ecosystem'}
        <div class="actions-inline"><label>Documentation version <select value={latest ? 'latest' : 'installed'} onchange={(event) => run(() => changeVersion(event.currentTarget.value === 'latest'))}><option value="installed">For your installed version</option><option value="latest">Latest available offline</option></select></label><button type="button" disabled={fetchBusy[readerApp]} onclick={() => run(fetchLatest)}>{fetchBusy[readerApp] ? 'Downloading…' : 'Download latest stable docs'}</button></div>
      {/if}
      {#if page?.needsFetch}<p class="setting-description">This bundled guide is for another version. Download your installed version’s docs below.</p>{/if}
      {#if pages.length}
        <div class="docs-reader-body">
          <nav aria-label="Guide pages"><ul>{#each pages as entry (entry.path)}<li><button type="button" class="text-action" class:current={page?.path === entry.path} onclick={() => run(() => openReader(readerApp!, entry.path))}>{entry.title}</button></li>{/each}</ul></nav>
          {#if page}<article class="markdown-body" use:docsNavigation>{@html page.html}</article>{/if}
        </div>
      {/if}
    </section>
  {/if}

  <section class="feature-section" aria-labelledby="learn-apps-heading">
    <div class="section-heading"><div><h2 id="learn-apps-heading">App guides</h2><p class="setting-description">Documentation links come from Arcade’s app catalog.</p></div><button type="button" disabled={loading} onclick={() => run(loadApps)}>{loading ? 'Loading…' : 'Refresh list'}</button></div>
    {#if apps.length}
      <div class="learn-app-list">
        {#each apps as app (app.id)}
          <div class="learn-app-row">
            <div class="learn-app-info"><strong>{app.name}</strong><span>{app.installedVersion ? `Installed v${app.installedVersion}` : 'Not installed'} · {app.available ? `${app.source} docs${app.docsVersion ? ` · v${app.docsVersion}` : ''}` : 'No local docs yet'}</span></div>
            <div class="actions-inline">
              {#if app.installedVersion && app.needsFetch}<button type="button" disabled={fetchBusy[app.id]} onclick={() => run(() => fetchInstalledDocs(app), app.name)}>{fetchBusy[app.id] ? 'Downloading…' : 'Download installed-version docs'}</button>{/if}
              {#if app.available}<button type="button" class="primary-action" onclick={() => run(() => openReader(app.id), app.name)}>Read here</button>{/if}
              <button type="button" class:primary-action={!app.available} onclick={() => run(() => api.openDocs(app.id), app.name)}>Open guide online</button>
            </div>
          </div>
        {/each}
      </div>
    {:else if !loading}<p class="setting-description">No app guides are available yet.</p>{/if}
  </section>
</div>
