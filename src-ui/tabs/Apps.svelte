<script lang="ts">
  import { tick } from 'svelte';
  import AppCard from '../components/AppCard.svelte';
  import AppDetail from '../components/AppDetail.svelte';
  import type { AppView, Channel, Operation, Release } from '../lib/api';
  import type { CatalogApp, CatalogBundle, CatalogData } from '../lib/catalog';
  import type { ShortcutsIndex } from '../lib/shortcuts';
  import { ui } from '../lib/state.svelte';

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
    <header class="page-heading">
      <div>
        <p class="eyebrow">ARCADE TOOLS / APPS</p>
        <h1>Arcade apps</h1>
        <p class="intro">Browse installed apps, discover what’s available, and choose a bundle to get started.</p>
      </div>
      <div class="catalog-heading-actions">
        <span class="app-count">{catalogApps.length} apps</span>
        <button class="small-button" type="button" disabled={catalogLoading || busy} onclick={() => run(onCatalogRefresh, 'Catalog')}>
          {catalogLoading ? 'Refreshing…' : 'Refresh catalog'}
        </button>
      </div>
    </header>

    {#if bundles.length}
      <section class="catalog-section bundles-section" aria-labelledby="bundles-title">
        <div class="catalog-section-heading"><div><p class="eyebrow">START WITH A SET</p><h2 id="bundles-title">Bundles</h2></div><span>{bundles.length} bundles</span></div>
        <div class="bundle-grid">
          {#each bundles as bundle (bundle.id)}
            <article class="bundle-card">
              <div><h3>{bundle.title}</h3><p>{bundle.description}</p></div>
              <span class="bundle-count">{bundle.apps === '*' ? catalogApps.length : bundle.apps.length} apps</span>
              <button class="small-button" type="button" onclick={() => viewBundle(bundle)}>View bundle</button>
            </article>
          {/each}
        </div>
      </section>
    {/if}

    <label class="setting-control">Category
      <select bind:value={category}><option value="all">All categories</option>{#each categories as item (item)}<option value={item}>{item}</option>{/each}</select>
    </label>

    <section class="catalog-section" aria-labelledby="installed-apps-title">
      <div class="catalog-section-heading"><div><p class="eyebrow">YOUR ARCADE</p><h2 id="installed-apps-title">Installed</h2></div><span>{installedApps.length}</span></div>
      {#if installedApps.length}
        <div class="app-list" aria-label="Installed Arcade apps">
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
      {:else if ui.loaded}
        <p class="catalog-empty-note">No Arcade apps are installed yet.</p>
      {:else}
        <p class="catalog-empty-note">Loading your apps…</p>
      {/if}
    </section>

    <section class="catalog-section" aria-labelledby="available-apps-title">
      <div class="catalog-section-heading"><div><p class="eyebrow">FROM THE CATALOG</p><h2 id="available-apps-title">Available</h2></div><span>{availableApps.length}</span></div>
      {#if availableApps.length}
        <div class="app-list" aria-label="Available Arcade apps">
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
      {:else if ui.loaded}
        <p class="catalog-empty-note">Every app in the catalog is installed.</p>
      {/if}
    </section>
  {/if}
</div>

<dialog bind:this={bundleDialog} class="bundle-dialog" onclose={closeBundleDialog} aria-labelledby="bundle-dialog-title">
  {#if selectedBundle}
    <p class="eyebrow">ARCADE BUNDLE</p>
    <h2 id="bundle-dialog-title">{selectedBundle.title}</h2>
    <p>{selectedBundle.description}</p>
    <fieldset class="bundle-selection">
      <legend>Select the apps to install</legend>
      {#each bundleApps as app (app.id)}
        <label class="bundle-choice">
          <input type="checkbox" checked={selectedBundleApps.includes(app.id)} disabled={busy || !canInstallFromCatalog(app)} onchange={(event) => toggleBundleApp(app.id, event.currentTarget.checked)} />
          <span><strong>{app.name}</strong><small>{app.pitch}{(app.installReason ?? appView(app.id)?.installReason) ? ` · ${app.installReason ?? appView(app.id)?.installReason}` : ''}</small></span>
          {#if isInstalled(app.id)}<span class="bundle-installed">Installed</span>{/if}
        </label>
      {/each}
    </fieldset>
    <div class="dialog-actions">
      <button type="button" onclick={() => bundleDialog.close()}>Cancel</button>
      <button class="primary" type="button" disabled={busy || !selectedBundleApps.length} onclick={confirmBundle}>Continue</button>
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
      <button type="button" onclick={() => bundleConfirmDialog.close()}>Cancel</button>
      <button class="primary" type="button" onclick={installConfirmedBundle}>Confirm installation</button>
    </div>
  {/if}
</dialog>
