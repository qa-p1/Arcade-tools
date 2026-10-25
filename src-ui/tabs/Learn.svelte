<script lang="ts">
  import { api, type DocPage, type DocsSearchHit, type LearnApp } from '../lib/api';
  import { onDestroy } from 'svelte';
  import Icon from '../components/Icon.svelte';

  type Props = { active: boolean; target: { id: string; serial: number }; run: (action: () => Promise<void>, label?: string) => void };
  let { active, target, run }: Props = $props();
  let readerApp = $state<string | null>(null);
  let pages = $state<{ path: string; title: string }[]>([]);
  let page = $state<DocPage | null>(null);
  let readerError = $state<string | null>(null);
  let latest = $state(false);
  let openedTarget = 0;
  let apps = $state<LearnApp[]>([]);
  let hits = $state<DocsSearchHit[]>([]);
  let query = $state('');
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let fetchBusy = $state<Record<string, boolean>>({});
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  // Responses can arrive out of order; only the latest request may apply.
  let searchGeneration = 0;
  let readerGeneration = 0;
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
    const generation = ++searchGeneration;
    if (term.length < 2) { hits = []; return; }
    loading = true;
    error = null;
    try {
      const found = await api.searchDocs(term);
      if (generation === searchGeneration) hits = found;
    }
    catch (problem) { if (generation === searchGeneration) error = problem instanceof Error ? problem.message : String(problem); }
    finally { if (generation === searchGeneration) loading = false; }
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
    const generation = ++readerGeneration;
    const current = () => generation === readerGeneration;
    try {
      if (readerApp !== id) { latest = false; readerApp = id; page = null; pages = []; }
      const useLatest = latest;
      const found = await api.docsPages(id, useLatest);
      if (!current()) return;
      pages = found;
      const chosen = path ?? found[0]?.path;
      const opened = chosen ? await api.readDoc(id, chosen, useLatest) : null;
      if (!current()) return;
      page = opened;
      if (!chosen) readerError = 'No pages are available offline for this app yet.';
    } catch (problem) { if (current()) readerError = problem instanceof Error ? problem.message : String(problem); }
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
    if (active && !loaded && !loading && !error) void loadApps();
  });
  function closeReader(): void { readerApp = null; page = null; readerError = null; }
  const readerName = $derived(apps.find((app) => app.id === readerApp)?.name ?? (readerApp === 'arcade.ecosystem' ? 'Arcade' : readerApp ?? ''));
  const searching = $derived(query.trim().length >= 2);
  // Other tabs open Learn on a specific app's guide.
  $effect(() => {
    if (active && target.id && target.serial !== openedTarget) { openedTarget = target.serial; void openReader(target.id); }
  });
</script>

<div id="panel-learn" class="tab-panel" role="tabpanel" aria-labelledby="tab-learn" tabindex="0" hidden={!active}>
  {#if readerApp}
    <section class="docs-reader" aria-labelledby="learn-reader-heading">
      <button class="btn ghost sm back-button" type="button" onclick={closeReader}>← All guides</button>
      <header class="page-head reader-head">
        <div>
          <h1 id="learn-reader-heading">{readerName} guide</h1>
          {#if page}<p class="sub">Version {page.version} · {page.source === 'bundled' ? 'included with Tools' : page.source}</p>{/if}
        </div>
        {#if readerApp !== 'arcade.ecosystem'}
          <div class="page-actions">
            <select class="select sm" aria-label="Documentation version" value={latest ? 'latest' : 'installed'} onchange={(event) => run(() => changeVersion(event.currentTarget.value === 'latest'))}><option value="installed">Your version</option><option value="latest">Latest saved</option></select>
            <button class="btn sm" type="button" disabled={fetchBusy[readerApp]} onclick={() => run(fetchLatest)}>{fetchBusy[readerApp] ? 'Downloading…' : 'Download latest'}</button>
          </div>
        {/if}
      </header>
      {#if readerError}<div class="notice warn"><Icon name="alert" /><span class="row-main">{readerError}</span></div>{/if}
      {#if page?.needsFetch}<div class="notice"><Icon name="info" /><span class="row-main">This guide was written for another version. Download the docs for your version for an exact match.</span></div>{/if}
      {#if pages.length}
        <div class="reader-body">
          <nav aria-label="Guide pages">
            <ul>{#each pages as entry (entry.path)}<li><button type="button" class:current={page?.path === entry.path} aria-current={page?.path === entry.path ? 'page' : undefined} onclick={() => run(() => openReader(readerApp!, entry.path))}>{entry.title}</button></li>{/each}</ul>
          </nav>
          {#if page}<div class="markdown-body reader-page" use:docsNavigation>{@html page.html}</div>{:else}<div class="reader-page"><div class="skeleton line"></div></div>{/if}
        </div>
      {/if}
    </section>
  {:else}
    <header class="page-head">
      <div>
        <h1>Learn</h1>
        <p class="sub">Guides for every Arcade app, readable offline.</p>
      </div>
    </header>

    <form class="search-form" role="search" onsubmit={(event) => { event.preventDefault(); void searchDocs(); }}>
      <label class="search-field">
        <Icon name="search" />
        <span class="visually-hidden">Search docs</span>
        <input id="docs-query" type="search" bind:value={query} placeholder="Search the guides — try “shortcut” or “export”" oninput={queueSearch} />
        {#if loading && searching}<span class="faint searching">Searching…</span>{/if}
      </label>
    </form>
    {#if error}<p class="error-text form-error">{error}</p>{/if}

    {#if searching}
      <section class="section" aria-label="Documentation search results">
        {#if hits.length}
          <ul class="list docs-results">
            {#each hits as hit (`${hit.app}:${hit.version}:${hit.path}`)}
              <li><button type="button" class="row docs-hit" onclick={() => run(() => openReader(hit.app, hit.path))}>
                <span class="glyph-tile sm"><img src="/glyphs/{hit.app}.svg" alt="" width="15" height="15" /></span>
                <span class="row-main"><span class="row-title">{hit.title}</span>{#if hit.snippet}<span class="row-sub">{hit.snippet}</span>{/if}</span>
                <span class="faint">{apps.find((app) => app.id === hit.app)?.name ?? hit.app}</span>
                <Icon name="chevronRight" />
              </button></li>
            {/each}
          </ul>
        {:else if !loading}
          <p class="muted no-results">No pages match “{query.trim()}”.</p>
        {/if}
      </section>
    {/if}

    <section class="section" aria-labelledby="learn-apps-heading">
      <div class="section-head"><h2 id="learn-apps-heading">Guides</h2><button type="button" class="btn ghost sm" disabled={loading} onclick={() => run(loadApps)}>{loading ? 'Loading…' : 'Refresh'}</button></div>
      {#if apps.length}
        <div class="guide-grid">
          <div class="card learn-app-row guide featured">
            <span class="glyph-tile"><img src="/glyphs/arcade.tools.svg" alt="" width="18" height="18" /></span>
            <div class="row-main"><div class="row-title">Getting started with Arcade</div><div class="row-sub">What the apps do and how they work together</div></div>
            <button type="button" class="btn sm primary" onclick={() => run(() => openReader('arcade.ecosystem'))}>Read</button>
          </div>
          {#each apps as app (app.id)}
            <div class="card learn-app-row guide">
              <span class="glyph-tile"><img src="/glyphs/{app.id}.svg" alt="" width="18" height="18" /></span>
              <div class="row-main">
                <div class="row-title">{app.name}</div>
                <div class="row-sub">{app.available ? `Offline${app.docsVersion ? ` · v${app.docsVersion}` : ''}` : 'Online only'}{app.installedVersion && app.needsFetch ? ' · older than your version' : ''}</div>
              </div>
              <div class="row-end">
                {#if app.installedVersion && app.needsFetch}<button type="button" class="btn ghost icon-only sm" title="Download the docs for v{app.installedVersion}" aria-label="Download installed-version docs" disabled={fetchBusy[app.id]} onclick={() => run(() => fetchInstalledDocs(app), app.name)}><Icon name="updates" size={15} /></button>{/if}
                {#if app.available}<button type="button" class="btn sm" onclick={() => run(() => openReader(app.id), app.name)}>Read here</button>{/if}
                <button type="button" class="btn ghost icon-only sm" title="Open the guide online" aria-label="Open guide online" onclick={() => run(() => api.openDocs(app.id), app.name)}><Icon name="external" size={15} /></button>
              </div>
            </div>
          {/each}
        </div>
      {:else if loading}
        <div class="list">{#each [0, 1, 2] as item (item)}<div class="skeleton-row"><span class="glyph-tile skeleton"></span><div class="row-main"><div class="skeleton line skel"></div></div></div>{/each}</div>
      {:else}
        <p class="muted">No guides are available yet.</p>
      {/if}
    </section>
  {/if}
</div>

<style>
  .back-button { margin: -6px 0 14px -8px; }
  .reader-head { margin-bottom: 16px; }
  .reader-head + .notice, .notice + .reader-body, .reader-head + .reader-body { margin-top: 0; }
  .notice { margin-bottom: 12px; }
  .reader-body { display: grid; grid-template-columns: 200px minmax(0, 1fr); gap: 32px; align-items: start; padding-top: 8px; border-top: 1px solid var(--line); }
  nav { position: sticky; top: 16px; padding-top: 12px; }
  nav ul { display: grid; gap: 1px; margin: 0; padding: 0; list-style: none; }
  nav button { width: 100%; padding: 6px 10px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); font-size: 12.5px; text-align: left; cursor: pointer; }
  nav button:hover { background: var(--hover); color: var(--text); }
  nav button.current { background: var(--sunken); color: var(--text); font-weight: 500; box-shadow: inset 2px 0 0 var(--accent); }
  .reader-page { max-width: 68ch; padding-top: 12px; }
  .search-form { margin-bottom: 4px; }
  .searching { font-size: 12px; }
  .form-error { margin-top: 8px; }
  .docs-results { margin: 0; padding: 0; list-style: none; }
  .docs-results li + li { border-top: 1px solid var(--line); }
  .docs-hit { width: 100%; border: 0; background: transparent; color: var(--text); text-align: left; cursor: pointer; }
  .docs-hit:hover { background: var(--sunken); }
  .docs-hit .row-title, .docs-hit .row-sub { display: block; }
  .docs-hit .row-sub { display: -webkit-box; overflow: hidden; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  .docs-hit > :global(.icon) { color: var(--faint); }
  .no-results { padding: 8px 2px; }
  .guide-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 10px; }
  .guide { display: flex; align-items: center; gap: 12px; padding: 12px 12px 12px 14px; }
  .guide + .guide { margin-top: 0; }
  .guide.featured { grid-column: 1 / -1; background: var(--sunken); }
  .skel { width: 40%; }
  @media (max-width: 760px) { .reader-body { grid-template-columns: 1fr; } nav { position: static; } }
</style>
