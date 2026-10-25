<script lang="ts">
  import TraySettings from '../components/TraySettings.svelte';
  import Icon from '../components/Icon.svelte';
  import { confirmOperation } from '../lib/controller';
  import { api, type AppView, type Bundle } from '../lib/api';
  import type { ShellTab } from '../lib/tabs.svelte';
  import type { ShortcutsIndex } from '../lib/shortcuts';

  type Props = {
    active: boolean;
    apps: AppView[];
    onInstall: (app: AppView) => Promise<void>;
    onComplete: () => void;
    onSelectTab: (tab: ShellTab) => void;
    shortcuts: ShortcutsIndex | null;
    run: (action: () => Promise<void>, label?: string) => void;
  };
  let { active, apps, onInstall, onComplete, onSelectTab, shortcuts, run }: Props = $props();
  let bundles = $state<Bundle[]>([]);
  let step = $state<'welcome' | 'choose' | 'shortcuts' | 'tray' | 'review' | 'done'>('welcome');
  let selected = $state<Set<string>>(new Set());
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  const selectedApps = $derived(apps.filter((app) => selected.has(app.id)));
  const steps = ['choose', 'shortcuts', 'tray', 'review'] as const;
  const stepIndex = $derived(steps.indexOf(step as typeof steps[number]));

  async function loadBundles(): Promise<void> {
    if (loaded || loading) return;
    loading = true;
    error = null;
    try { bundles = await api.listBundles(); loaded = true; }
    catch (problem) { error = problem instanceof Error ? problem.message : String(problem); }
    finally { loading = false; }
  }

  function bundleIds(bundle: Bundle): string[] {
    if (typeof bundle.apps === 'string') return bundle.apps === '*' ? apps.map((app) => app.id) : [];
    return bundle.apps;
  }

  function chooseBundle(bundle: Bundle): void {
    selected = new Set(bundleIds(bundle));
  }

  function toggle(id: string, checked: boolean): void {
    const next = new Set(selected);
    if (checked) next.add(id); else next.delete(id);
    selected = next;
  }

  function finish(): void {
    try { localStorage.setItem('arcade-tools-onboarding-complete', 'yes'); } catch { /* finish for this session */ }
    // The preference is the durable record; the local flag only covers a failed write.
    run(() => api.completeOnboarding());
    step = 'done';
  }

  $effect(() => { if (active) void loadBundles(); });
</script>

{#snippet progress()}
  <div class="steps" aria-label="Step {stepIndex + 1} of 4">{#each steps as item, i (item)}<span class:done={i <= stepIndex}></span>{/each}</div>
{/snippet}

<div id="panel-onboarding" class="tab-panel" role="tabpanel" aria-labelledby="tab-onboarding" tabindex="0" hidden={!active}>
  {#if step === 'welcome'}
    <div class="welcome">
      <span class="glyph-tile lg"><img src="/glyphs/arcade.tools.svg" alt="" width="26" height="26" /></span>
      <h1>One home for your Arcade apps</h1>
      <p class="lede">Install the apps you want, keep them up to date and see how they work together. Tools asks before every install.</p>
      <div class="btn-group"><button type="button" class="btn primary" onclick={() => step = 'choose'}>Choose apps</button><button type="button" class="btn ghost" onclick={finish}>Skip setup</button></div>
    </div>
  {:else if step === 'choose'}
    {@render progress()}
    <header class="page-head">
      <div><h1>What would you like to try?</h1><p class="sub">Pick a bundle or single apps. Nothing installs until you confirm.</p></div>
      <button type="button" class="btn ghost" onclick={finish}>Skip setup</button>
    </header>
    {#if error}<p class="status-warning">Could not load bundles: {error}</p>{/if}
    {#if bundles.length}
      <section class="section">
        <div class="section-head"><h2>Bundles</h2></div>
        <div class="bundle-list">{#each bundles as bundle (bundle.id)}<button class="card bundle-card" type="button" onclick={() => chooseBundle(bundle)}><strong>{bundle.title}</strong><span>{bundle.description}</span><small>{bundleIds(bundle).length} apps</small></button>{/each}</div>
      </section>
    {/if}
    <section class="section">
      <div class="section-head"><h2>Apps</h2><span class="meta">{selected.size} selected</span></div>
      <div class="list">{#each apps as app (app.id)}<label class="row"><input type="checkbox" checked={selected.has(app.id)} onchange={(event) => toggle(app.id, event.currentTarget.checked)} /><span class="glyph-tile sm"><img src="/glyphs/{app.id}.svg" alt="" width="15" height="15" /></span><span class="row-main"><span class="row-title">{app.name}</span><span class="row-sub">{app.pitch}</span></span>{#if app.installed || app.managed}<span class="badge">Installed</span>{:else if !app.installSupported}<span class="badge">Not available</span>{/if}</label>{/each}</div>
    </section>
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'welcome'}>Back</button><button type="button" class="btn primary" disabled={selected.size === 0} onclick={() => step = 'shortcuts'}>Continue with {selected.size}</button></div>
  {:else if step === 'shortcuts'}
    {@render progress()}
    <header class="page-head"><div><h1>Shortcuts</h1><p class="sub">{shortcuts?.conflicts.length ? `${shortcuts.conflicts.length} shortcuts clash. You can sort them out now or later.` : 'No shortcuts clash on this computer.'}</p></div></header>
    {#if shortcuts?.conflicts.length}<ul class="list plain">{#each shortcuts.conflicts as conflict, i (`${conflict.chord}-${i}`)}<li class="row"><span class="dot warn"></span><span class="row-main">{conflict.a.label} <span class="faint">and</span> {conflict.b.label}</span><kbd class="key-chip">{conflict.chord}</kbd></li>{/each}</ul>{/if}
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'choose'}>Back</button><span class="spacer"></span><button type="button" class="btn" onclick={() => onSelectTab('shortcuts')}>Open Shortcuts</button><button type="button" class="btn primary" onclick={() => step = 'tray'}>Continue</button></div>
  {:else if step === 'tray'}
    {@render progress()}
    <header class="page-head"><div><h1>Tray and overlay</h1><p class="sub">One tray menu for every app, and a shortcut sheet on demand.</p></div></header>
    <TraySettings active={active && step === 'tray'} busy={false} />
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'shortcuts'}>Back</button><button type="button" class="btn primary" onclick={() => step = 'review'}>Continue</button></div>
  {:else if step === 'review'}
    {@render progress()}
    <header class="page-head"><div><h1>Review and install</h1><p class="sub">Each install opens its own confirmation.</p></div></header>
    <div class="list">{#each selectedApps as app (app.id)}<div class="row"><span class="glyph-tile sm"><img src="/glyphs/{app.id}.svg" alt="" width="15" height="15" /></span><span class="row-main"><span class="row-title">{app.name}</span><span class="row-sub">{app.pitch}</span></span>{#if app.installed || app.managed}{#if !app.managed && app.installationClass !== 'dev'}<button type="button" class="btn sm" onclick={() => run(() => confirmOperation(app, 'adopt'), app.name)}>Let Tools manage it</button>{:else}<span class="badge">Installed</span>{/if}{:else if !app.installSupported}<span class="badge">Unavailable</span>{:else}<button type="button" class="btn sm primary" onclick={() => run(() => onInstall(app), app.name)}>Review &amp; install</button>{/if}</div>{/each}</div>
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'choose'}>Change selection</button><button type="button" class="btn primary" onclick={finish}>Finish setup</button></div>
  {:else}
    <div class="welcome">
      <span class="glyph-tile lg done"><Icon name="check" size={24} /></span>
      <h1>You're all set</h1>
      <p class="lede">Your apps are in Apps. You can run this setup again from Settings.</p>
      {#if shortcuts}<p class="tip muted">Press <kbd class="key-chip">{shortcuts.os === 'windows' ? 'Ctrl+Alt+/' : shortcuts.os === 'macos' ? 'Ctrl+Option+/' : 'Super+/'}</kbd> in any app to see its shortcuts.</p>{/if}
      <div class="btn-group"><button type="button" class="btn primary" onclick={onComplete}>Go to Home</button><button type="button" class="btn ghost" onclick={() => onSelectTab('learn')}>Read the guide</button><button type="button" class="btn ghost" onclick={() => step = 'choose'}>Choose more apps</button></div>
    </div>
  {/if}
</div>

<style>
  .welcome { display: grid; justify-items: start; gap: 12px; max-width: 520px; padding: 48px 0 24px; }
  .welcome h1 { margin-top: 8px; font-size: 26px; font-weight: 600; letter-spacing: -.02em; }
  .lede { color: var(--muted); font-size: 14px; line-height: 1.6; }
  .welcome .btn-group { margin-top: 12px; }
  .done { color: var(--accent); }
  .tip { display: flex; align-items: center; gap: 6px; }
  .steps { display: flex; gap: 6px; margin-bottom: 20px; }
  .steps span { width: 28px; height: 3px; border-radius: 99px; background: var(--line-strong); }
  .steps span.done { background: var(--accent); }
  .bundle-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 10px; }
  .bundle-card { display: grid; grid-template-columns: minmax(0, 1fr); align-content: start; gap: 4px; margin: 0; color: var(--text); text-align: left; white-space: normal; overflow-wrap: anywhere; cursor: pointer; }
  .bundle-card + .bundle-card { margin-top: 0; }
  .bundle-card:hover { background: var(--sunken); }
  .bundle-card span { color: var(--muted); font-size: 12.5px; }
  .bundle-card small { color: var(--faint); font-size: 12px; }
  label.row .row-title, label.row .row-sub { display: block; }
  .plain { margin: 0; padding: 0; list-style: none; }
  .footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: 24px; }
  .spacer { flex: 1; }
</style>
