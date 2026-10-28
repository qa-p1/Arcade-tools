<script lang="ts">
  import { glyph } from '../lib/glyphs';
  import { tick } from 'svelte';
  import AppCard from '../components/AppCard.svelte';
  import AppDetail from '../components/AppDetail.svelte';
  import type { AppView, Channel, Operation, Release } from '../lib/api';
  import type { CatalogApp, CatalogBundle, CatalogData } from '../lib/catalog';
  import type { ShortcutsIndex } from '../lib/shortcuts';
  import { ui } from '../lib/state.svelte';
  import Icon from '../components/Icon.svelte';

  type Props = {
    active: boolean;
    apps: AppView[];
    selected: string | null;
    releases: Map<string, Release>;
    releaseNotesHtml: Record<string, string>;
    catalogData: CatalogData | null;
    catalogLoading: boolean;
    shortcuts: ShortcutsIndex | null;
    busy: boolean;
    onChannel: (app: AppView, channel: Channel) => Promise<void>;
    onLogin: (app: AppView, enabled: boolean) => Promise<void>;
    onLaunch: (app: AppView, mode: 'foreground' | 'background') => Promise<void>;
    onInstall: (app: AppView) => Promise<void>;
    onCheckUpdates: (app: AppView) => Promise<void>;
    onOperation: (app: AppView, operation: Operation) => Promise<void>;
    onReleases: (app: AppView) => Promise<void>;
    onCatalogRefresh: () => Promise<void>;
    onCheckLatest: (id: string) => Promise<void>;
    onOpenDocs: (id: string) => Promise<void>;
    onInstallCatalogApp: (id: string) => Promise<void>;
    onUpdateCatalogApp: (id: string) => Promise<void>;
    onUninstallCatalogApp: (id: string) => Promise<void>;
    onInstallBundle: (bundleId: string, appIds: string[]) => Promise<void>;
    run: (action: () => Promise<void>, appName: string) => void;
  };

  let {
    active, apps, selected, releases, releaseNotesHtml, catalogData, catalogLoading, shortcuts, busy,
    onChannel, onLogin, onLaunch, onInstall, onCheckUpdates, onOperation, onReleases,
    onCatalogRefresh, onCheckLatest, onOpenDocs, onInstallCatalogApp, onUpdateCatalogApp,
    onUninstallCatalogApp, onInstallBundle, run,
  }: Props = $props();

  let detailId = $state<string | null>(null);
  let category = $state('all');
  let selectedBundle = $state<CatalogBundle | null>(null);
  let selectedBundleApps = $state<string[]>([]);
  let bundleDialog: HTMLDialogElement;
  let bundleConfirmDialog: HTMLDialogElement;
  let pendingBundle = $state<{ bundle: CatalogBundle; appIds: string[] } | null>(null);

  const catalogApps = $derived(catalogData?.catalog.apps ?? apps.map((app): CatalogApp => ({
    id: app.id,
    name: app.name,
    repo: app.releasesUrl.replace(/^https:\/\/github\.com\//, '').replace(/\/releases$/, ''),
    pitch: app.pitch,
    tagline: app.tagline,
    description: app.description,
    category: app.category,
    icon: app.icon,
    screenshots: [],
    platforms: app.platforms,
    linkProtocol: [],
    minTools: app.minTools,
    docs: 'README.md',
    resident: false,
    homepage: app.releasesUrl.replace(/\/releases$/, ''),
  })));
  const categories = $derived([...new Set(catalogApps.map((app) => app.category))].sort());
  const filteredApps = $derived(catalogApps.filter((app) => category === 'all' || app.category === category));
  const installedApps = $derived(filteredApps.filter((app) => isInstalled(app.id)));
  const availableApps = $derived(filteredApps.filter((app) => !isInstalled(app.id)));
  const detailApp = $derived(catalogApps.find((app) => app.id === detailId) ?? null);
  const detailView = $derived(apps.find((app) => app.id === detailId));
  const bundles = $derived(catalogData?.catalog.bundles ?? []);
  const bundleApps = $derived(selectedBundle ? appsInBundle(selectedBundle) : []);

  function appsInBundle(bundle: CatalogBundle): CatalogApp[] {
    if (bundle.apps === '*') return catalogApps;
    const ids = new Set(bundle.apps);
    return catalogApps.filter((app) => ids.has(app.id));
  }

  function isInstalled(id: string): boolean {
    const app = apps.find((candidate) => candidate.id === id);
    return Boolean(app?.installed || app?.managed || app?.version);
  }

  function appView(id: string): AppView | undefined {
    return apps.find((app) => app.id === id);
  }

  function canInstallFromCatalog(app: CatalogApp): boolean {
    const view = appView(app.id);
    return Boolean(view?.installSupported && !(app.installReason ?? view.installReason) && !isInstalled(app.id));
  }

  function showDetails(app: AppView): void {
    detailId = app.id;
  }

  function backToList(): void {
    detailId = null;
  }

  async function viewBundle(bundle: CatalogBundle): Promise<void> {
    selectedBundle = bundle;
    selectedBundleApps = appsInBundle(bundle).filter(canInstallFromCatalog).map((app) => app.id);
    await tick();
    if (!bundleDialog.open) bundleDialog.showModal();
  }

  function closeBundleDialog(): void {
    selectedBundle = null;
    selectedBundleApps = [];
  }

  function toggleBundleApp(id: string, checked: boolean): void {
    selectedBundleApps = checked
      ? [...selectedBundleApps, id]
      : selectedBundleApps.filter((value) => value !== id);
  }

  function confirmBundle(): void {
    const bundle = selectedBundle;
    const appIds = [...selectedBundleApps];
    if (!bundle || !appIds.length) return;
    pendingBundle = { bundle, appIds };
    bundleDialog.close();
    void tick().then(() => bundleConfirmDialog.showModal());
  }

  function installConfirmedBundle(): void {
    const request = pendingBundle;
    pendingBundle = null;
    bundleConfirmDialog.close();
    if (!request) return;
    run(() => onInstallBundle(request.bundle.id, request.appIds), request.bundle.title);
  }

  function closeBundleConfirmation(): void {
    pendingBundle = null;
  }
</script>

<div id="panel-apps" class="tab-panel" role="tabpanel" aria-labelledby="tab-apps" tabindex="0" hidden={!active}>
  {#if detailApp}
    <AppDetail
      app={detailApp}
      installedApp={detailView}
      latest={releases.get(detailApp.id)}
      releaseNotesHtml={releaseNotesHtml[detailApp.id]}
      shortcuts={shortcuts}
      engines={catalogData?.engines.engines ?? []}
      {busy}
      onBack={backToList}
      onCheckLatest={() => onCheckLatest(detailApp.id)}
      onOpenDocs={() => onOpenDocs(detailApp.id)}
      onInstall={onInstallCatalogApp}
      onUpdate={onUpdateCatalogApp}
      onUninstall={onUninstallCatalogApp}
      {run}
    />
  {:else}
    <header class="page-head">
      <div>
        <h1>Apps</h1>
        <p class="sub">{catalogApps.length} apps in the catalog · {installedApps.length} installed</p>
      </div>
      <div class="page-actions">
        <button class="btn" class:busy={catalogLoading} type="button" disabled={catalogLoading || busy} onclick={() => run(onCatalogRefresh, 'Catalog')}>
          <Icon name="refresh" size={14} />Refresh catalog
        </button>
      </div>
    </header>

    {#if catalogData?.offline}
      <div class="notice warn" role="status"><Icon name="alert" /><span class="row-main">Couldn't reach the catalog, so the copy saved on {catalogData.catalog.updated} is shown.</span><button type="button" class="btn sm" disabled={catalogLoading || busy} onclick={() => run(onCatalogRefresh, 'Catalog')}>Retry</button></div>
    {/if}
    {#if categories.length > 1}
      <div class="segmented category-filter" role="group" aria-label="Category">
        <button type="button" class:active={category === 'all'} aria-pressed={category === 'all'} onclick={() => category = 'all'}>All</button>
        {#each categories as item (item)}<button type="button" class:active={category === item} aria-pressed={category === item} onclick={() => category = item}>{item}</button>{/each}
      </div>
    {/if}

    <section class="section" aria-labelledby="installed-apps-title">
      <div class="section-head"><h2 id="installed-apps-title">Installed</h2><span class="meta">{installedApps.length}</span></div>
      {#if installedApps.length}
        <div class="app-grid" aria-label="Installed Arcade apps">
          {#each installedApps as entry (entry.id)}
            {@const app = appView(entry.id)}
            {#if app}<AppCard
              {app}
              selected={selected === app.id}
              release={releases.get(app.id)}
              {busy}
              {onChannel}
              {onLogin}
              {onLaunch}
              {onInstall}
              {onCheckUpdates}
              {onOperation}
              {onReleases}
              onDetails={showDetails}
              {run}
            />{/if}
          {/each}
        </div>
      {:else if !ui.loaded}
        <div class="app-grid" aria-busy="true" aria-label="Loading your apps">{#each [0, 1, 2] as item (item)}<div class="card skeleton-card"><span class="glyph-tile skeleton"></span><div class="skeleton line"></div><div class="skeleton line short"></div></div>{/each}</div>
      {:else}
        <div class="empty-state"><Icon name="apps" size={24} /><h3>No Arcade apps installed yet</h3><p>Pick one below or install a bundle. Nothing installs until you review it.</p></div>
      {/if}
    </section>

    {#if availableApps.length}
      <section class="section" aria-labelledby="available-apps-title">
        <div class="section-head"><h2 id="available-apps-title">Available</h2><span class="meta">{availableApps.length}</span></div>
        <div class="app-grid" aria-label="Available Arcade apps">
          {#each availableApps as entry (entry.id)}
            {@const app = appView(entry.id)}
            {#if app}<AppCard
              {app}
              selected={selected === app.id}
              release={releases.get(app.id)}
              {busy}
              {onChannel}
              {onLogin}
              {onLaunch}
              {onInstall}
              {onCheckUpdates}
              {onOperation}
              {onReleases}
              onDetails={showDetails}
              {run}
            />{/if}
          {/each}
        </div>
      </section>
    {/if}

    {#if bundles.length}
      <section class="section" aria-labelledby="bundles-title">
        <div class="section-head"><h2 id="bundles-title">Bundles</h2><span class="meta">Install a few apps that work well together</span></div>
        <div class="bundle-grid">
          {#each bundles as bundle (bundle.id)}
            <button class="card bundle-card" type="button" onclick={() => viewBundle(bundle)}>
              <span class="bundle-title">{bundle.title}<span class="faint">{bundle.apps === '*' ? catalogApps.length : bundle.apps.length} apps</span></span>
              <span class="bundle-copy">{bundle.description}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}
  {/if}
</div>

<dialog bind:this={bundleDialog} class="bundle-dialog" onclose={closeBundleDialog} aria-labelledby="bundle-dialog-title">
  {#if selectedBundle}
    <h2 id="bundle-dialog-title">{selectedBundle.title}</h2>
    <p>{selectedBundle.description}</p>
    <fieldset class="dialog-list">
      <legend>Choose the apps to install</legend>
      {#each bundleApps as app (app.id)}
        {@const reason = app.installReason ?? appView(app.id)?.installReason}
        <label class="row">
          <input type="checkbox" checked={selectedBundleApps.includes(app.id)} disabled={busy || !canInstallFromCatalog(app)} onchange={(event) => toggleBundleApp(app.id, event.currentTarget.checked)} />
          <span class="glyph-tile sm"><img src={glyph(app.id)} alt="" width="15" height="15" /></span>
          <span class="row-main"><span class="row-title">{app.name}</span><span class="row-sub">{reason ?? app.tagline}</span></span>
          {#if isInstalled(app.id)}<span class="badge">Installed</span>{/if}
        </label>
      {/each}
    </fieldset>
    <div class="dialog-actions">
      <button class="btn" type="button" onclick={() => bundleDialog.close()}>Cancel</button>
      <button class="btn primary" type="button" disabled={busy || !selectedBundleApps.length} onclick={confirmBundle}>Continue</button>
    </div>
  {/if}
</dialog>

<dialog bind:this={bundleConfirmDialog} class="catalog-confirm" onclose={closeBundleConfirmation} aria-labelledby="bundle-confirm-title">
  {#if pendingBundle}
    <h2 id="bundle-confirm-title">Install {pendingBundle.appIds.length} apps from {pendingBundle.bundle.title}?</h2>
    <p>Arcade Tools will install the selected apps for your user account.</p>
    <ul class="bundle-confirm-list">
      {#each pendingBundle.appIds as id (id)}<li>{catalogApps.find((app) => app.id === id)?.name ?? id}</li>{/each}
    </ul>
    <div class="dialog-actions">
      <button class="btn" type="button" onclick={() => bundleConfirmDialog.close()}>Cancel</button>
      <button class="btn primary" type="button" onclick={installConfirmedBundle}>Install</button>
    </div>
  {/if}
</dialog>

<style>
  .skeleton-card { display: grid; gap: 10px; }
  .skeleton-card .short { width: 60%; }
  .category-filter { margin-bottom: 4px; }
  .category-filter button { text-transform: capitalize; }
  .category-filter + .section { margin-top: 24px; }
  .app-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 12px; }
  .bundle-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
  .bundle-card { display: grid; align-content: start; gap: 4px; margin: 0; color: var(--text); text-align: left; cursor: pointer; }
  .bundle-card:hover { border-color: var(--line-strong); background: var(--sunken); }
  .bundle-title { display: flex; justify-content: space-between; gap: 8px; font-weight: 600; }
  .bundle-title .faint { font-size: var(--fs-small); font-weight: 400; }
  .bundle-copy { color: var(--muted); font-size: var(--fs-control); }
  .bundle-dialog { width: min(520px, calc(100vw - 32px)); }
  .bundle-confirm-list { margin: 12px 0 0; padding-left: 20px; color: var(--muted); }
</style>
