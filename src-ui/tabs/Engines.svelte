<script lang="ts">
  import { api, type EngineView } from '../lib/api';

  type Props = { active: boolean; run: (action: () => Promise<void>, label?: string) => void };
  let { active, run }: Props = $props();
  let engines = $state<EngineView[]>([]);
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  let copied = $state<string | null>(null);

  async function load(refresh = false): Promise<void> {
    loading = true;
    error = null;
    try { engines = await api.listEngines(refresh); loaded = true; }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  function statusText(engine: EngineView): string {
    switch (engine.status.status) {
      case 'found': return `Found${engine.status.version ? ` · v${engine.status.version}` : ''}`;
      case 'tooOld': return `Version ${engine.status.version ?? 'unknown'} is older than ${engine.status.minimum ?? engine.minimumVersion ?? 'required'}`;
      case 'missing': return 'Not found on PATH';
      default: return engine.status.message ?? 'Could not check this engine';
    }
  }

  async function copyHint(engine: EngineView): Promise<void> {
    if (!engine.installHint) return;
    await navigator.clipboard.writeText(engine.installHint);
    copied = engine.id;
    setTimeout(() => { if (copied === engine.id) copied = null; }, 1800);
  }

  $effect(() => { if (active && !loaded && !loading) void load(); });
</script>

<div id="panel-engines" class="tab-panel" role="tabpanel" aria-labelledby="tab-engines" tabindex="0" hidden={!active}>
  <header class="page-heading"><div><p class="eyebrow">ARCADE TOOLS / ENGINES</p><h1>Check the tools your apps use.</h1><p class="intro">Arcade detects optional engines and shows guidance for your platform. Tools does not install system packages.</p></div><button type="button" disabled={loading} onclick={() => run(() => load(true))}>{loading ? 'Checking…' : 'Refresh detection'}</button></header>

  {#if error}<div class="empty-state"><h2>Engine information is unavailable</h2><p>{error}</p><button type="button" onclick={() => run(() => load())}>Try again</button></div>
  {:else if engines.length}
    <div class="engine-list">
      {#each engines as engine (engine.id)}
        <section class="feature-card engine-card">
          <div class="engine-heading"><div><h2>{engine.name}</h2><p class="setting-description">{engine.binaries.join(' or ')}{engine.minimumVersion ? ` · minimum ${engine.minimumVersion}` : ''}</p></div><span class="badge" class:engine-found={engine.status.status === 'found'} class:engine-missing={engine.status.status !== 'found'}>{statusText(engine)}</span></div>
          {#if engine.status.path}<p class="engine-path">Detected at <code>{engine.status.path}</code></p>{/if}
          <div class="engine-uses"><h3>Used by</h3><ul>{#each engine.usedBy as usage (`${usage.app}:${usage.purpose}`)}<li><strong>{usage.app}</strong><span>{usage.purpose}</span></li>{/each}</ul></div>
          {#if engine.installHint}<div class="install-hint"><div><h3>Install guidance</h3><code>{engine.installHint}</code></div><button type="button" onclick={() => run(() => copyHint(engine))}>{copied === engine.id ? 'Copied' : 'Copy command'}</button></div>{/if}
          {#if engine.linkDownload}<p class="setting-description">Arcade Link reports a supported managed download for this engine where available. This page only detects and explains; it does not start a download.</p>{/if}
          <a class="text-action" href={engine.homepage} target="_blank" rel="noreferrer">Engine project ↗</a>
        </section>
      {/each}
    </div>
  {:else}<div class="empty-state"><p>{loading ? 'Checking engines…' : 'No optional engines are listed.'}</p></div>{/if}
</div>
