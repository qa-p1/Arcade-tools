<script lang="ts">
  import Icon from '../components/Icon.svelte';
  import { api, type EngineView } from '../lib/api';
  import { askForConfirmation, ui } from '../lib/state.svelte';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void };
  let { active, run }: Props = $props();
  let engines = $state<EngineView[]>([]);
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let copied = $state<string | null>(null);
  let downloading = $state<string | null>(null);
  let downloadMessage = $state('');
  const found = $derived(engines.filter((engine) => engine.status.status === 'found'));
  // Engines that need something from the user come first.
  const ordered = $derived([...engines.filter((engine) => engine.status.status !== 'found'), ...found]);
  const appName = (id: string) => ui.apps.find((app) => app.id === id)?.name.replace(/^Arcade /, '') ?? id.replace(/^arcade\./, '');

  async function download(engine: EngineView): Promise<void> {
    const choice = await askForConfirmation({ title: `Download ${engine.name}?`, copy: 'Arcade verifies the download before using it. On Windows, finish the installer that opens.', action: 'Download', destructive: false, kind: null, folders: [] });
    if (!choice.confirmed) return;
    downloading = engine.id;
    try {
      downloadMessage = await api.downloadEngine(engine.id);
      await load(true);
    } finally { downloading = null; }
  }

  async function load(refresh = false): Promise<void> {
    loading = true;
    error = null;
    try { engines = await api.listEngines(refresh); loaded = true; }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  function status(engine: EngineView): { text: string; tone: string } {
    switch (engine.status.status) {
      case 'found': return { text: engine.status.version ? `v${engine.status.version}` : 'Installed', tone: 'ok' };
      case 'tooOld': return { text: `Needs ${engine.status.minimum ?? engine.minimumVersion ?? 'a newer version'}`, tone: 'warn' };
      case 'missing': return { text: 'Not installed', tone: '' };
      default: return { text: 'Check failed', tone: 'danger' };
    }
  }

  async function copyHint(engine: EngineView): Promise<void> {
    if (!engine.installHint) return;
    await navigator.clipboard.writeText(engine.installHint);
    copied = engine.id;
    setTimeout(() => { if (copied === engine.id) copied = null; }, 1800);
  }

  // A failed check waits for Try again instead of retrying in a loop.
  $effect(() => { if (active && !loaded && !loading && !error) void load(); });
  // Checked quietly soon after start, so the page is ready when opened.
  $effect(() => {
    if (!ui.loaded || loaded) return;
    const timer = setTimeout(() => { if (!loaded && !loading && !error) void load(); }, 2500);
    return () => clearTimeout(timer);
  });
</script>

<div id="panel-engines" class="tab-panel" role="tabpanel" aria-labelledby="tab-engines" tabindex="0" hidden={!active}>
  <header class="page-head">
    <div>
      <h1>Engines</h1>
      <p class="sub">{loaded ? `${found.length} of ${engines.length} found · ` : ''}Optional programs that unlock extra features in some apps.</p>
    </div>
    <div class="page-actions">
      <button type="button" class="btn" disabled={loading} onclick={() => run(() => load(true))}><Icon name="refresh" size={14} />{loading ? 'Checking…' : 'Check again'}</button>
    </div>
  </header>

  {#if downloadMessage}<div class="notice ok" role="status"><Icon name="check" /><span class="row-main">{downloadMessage}</span></div>{/if}

  {#if error && !engines.length}
    <div class="empty-state">
      <Icon name="alert" size={24} />
      <h2>Engines could not be checked</h2>
      <p>{error}</p>
      <div class="btn-group"><button type="button" class="btn" onclick={() => run(() => load())}>Try again</button></div>
    </div>
  {:else if engines.length}
    <div class="list">
      {#each ordered as engine (engine.id)}
        {@const state = status(engine)}
        <section class="engine" aria-labelledby="engine-{engine.id}">
          <div class="row">
            <div class="row-main">
              <h2 class="row-title" id="engine-{engine.id}">{engine.name}</h2>
              <div class="row-sub">{engine.usedBy.map((use) => `${appName(use.app)}: ${use.for}`).join(' · ')}</div>
            </div>
            <span class="badge {state.tone}" title={engine.status.path ?? engine.status.message ?? ''}>{#if state.tone === 'ok'}<span class="dot ok"></span>{/if}{state.text}</span>
            <a class="btn ghost icon-only sm" href={engine.homepage} target="_blank" rel="noreferrer" title="Project website" aria-label="{engine.name} website"><Icon name="external" size={14} /></a>
          </div>
          {#if engine.status.status !== 'found' && (engine.installHint || engine.linkDownload || engine.status.message)}
            <div class="row-extra">
              {#if engine.status.status === 'error' && engine.status.message}<p class="status-warning">{engine.status.message}</p>{/if}
              {#if engine.installHint}
                <div class="hint"><code class="mono">{engine.installHint}</code><button type="button" class="btn ghost sm" onclick={() => run(() => copyHint(engine))}><Icon name={copied === engine.id ? 'check' : 'copy'} size={14} />{copied === engine.id ? 'Copied' : 'Copy'}</button></div>
              {/if}
              {#if engine.linkDownload}<button type="button" class="btn sm primary download" disabled={Boolean(downloading)} onclick={() => run(() => download(engine), engine.name)}>{downloading === engine.id ? 'Downloading…' : 'Download with Arcade'}</button>{/if}
            </div>
          {/if}
        </section>
      {/each}
    </div>
    <p class="section-note">Tools shows the install command for your system but never installs system packages itself.</p>
  {:else if loading || !loaded}
    <div class="list" aria-busy="true" aria-label="Checking engines">
      {#each [0, 1, 2, 3, 4] as item (item)}<div class="skeleton-row"><div class="row-main"><div class="skeleton line skel-a"></div><div class="skeleton line skel-b"></div></div><div class="skeleton line skel-c"></div></div>{/each}
    </div>
  {:else}
    <div class="empty-state"><Icon name="engine" size={24} /><h2>No engines listed</h2><p>None of your apps need an extra program.</p></div>
  {/if}
</div>

<style>
  .notice { margin-bottom: 12px; }
  .engine + .engine { border-top: 1px solid var(--line); }
  h2.row-title { font-size: 13px; font-weight: 600; }
  .row-extra { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding-left: 14px; }
  .hint { display: flex; flex: 1; align-items: center; gap: 8px; min-width: 0; padding: 4px 4px 4px 10px; border-radius: var(--radius-sm); background: var(--sunken); }
  .hint code { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .skel-a { width: 30%; }
  .skel-b { width: 55%; margin-top: 8px; }
  .skel-c { width: 70px; }
</style>
