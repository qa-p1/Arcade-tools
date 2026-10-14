<script lang="ts">
  import TraySettings from '../components/TraySettings.svelte';
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

<div id="panel-onboarding" class="tab-panel" role="tabpanel" aria-labelledby="tab-onboarding" tabindex="0" hidden={!active}>
  {#if step === 'welcome'}
    <header class="onboarding-welcome">
      <p class="eyebrow">ARCADE TOOLS / GETTING STARTED</p>
      <h1>One home for your Arcade apps.</h1>
      <p class="intro">Browse the catalog, keep apps up to date, and see how Arcade apps connect. Choose what you want to try; Tools will ask before every install.</p>
      <div class="onboarding-actions"><button type="button" class="primary-action" onclick={() => step = 'choose'}>Choose apps</button><button type="button" onclick={finish}>Skip setup</button></div>
    </header>
    <section class="onboarding-benefits"><div><strong>Choose a starting point</strong><span>Pick one app or a bundle from the catalog.</span></div><div><strong>Review each app</strong><span>Check its details and confirm before an install begins.</span></div><div><strong>Keep control</strong><span>Skip now and come back to Onboarding whenever you like.</span></div></section>
  {:else if step === 'choose'}
    <header class="page-heading"><div><p class="eyebrow">ARCADE TOOLS / ONBOARDING · 1 OF 4</p><h1>What would you like to try?</h1><p class="intro">A bundle selects apps for review. It does not install them.</p></div><button type="button" onclick={finish}>Skip setup</button></header>
    {#if error}<p class="status-warning">Could not load bundles: {error}</p>{/if}
    {#if bundles.length}<section class="feature-section"><div class="section-heading"><div><h2>Start with a bundle</h2><p class="setting-description">Choose a group curated for a common task.</p></div></div><div class="bundle-list">{#each bundles as bundle (bundle.id)}<button class="bundle-card" type="button" onclick={() => chooseBundle(bundle)}><strong>{bundle.title}</strong><span>{bundle.description}</span><small>{bundleIds(bundle).length} apps</small></button>{/each}</div></section>{/if}
    <section class="feature-section"><div class="section-heading"><div><h2>Or choose apps</h2><p class="setting-description">Already installed apps stay selected only for review; they will not be installed again.</p></div><span class="app-count">{selected.size} selected</span></div>
      <div class="onboarding-app-list">{#each apps as app (app.id)}<label class="onboarding-app-row"><input type="checkbox" checked={selected.has(app.id)} onchange={(event) => toggle(app.id, event.currentTarget.checked)} /><img class="app-glyph" src="/glyphs/{app.id}.svg" alt="" width="30" height="30" /><span class="onboarding-app-info"><strong>{app.name}</strong><small>{app.pitch}</small></span>{#if app.installed || app.managed}<span class="badge">Already installed</span>{:else if !app.installSupported}<span class="badge">Not installable by this Tools version</span>{/if}</label>{/each}</div>
    </section>
    <div class="onboarding-footer"><button type="button" onclick={() => step = 'welcome'}>Back</button><button type="button" class="primary-action" disabled={selected.size === 0} onclick={() => step = 'shortcuts'}>Review {selected.size} selected</button></div>
  {:else if step === 'shortcuts'}
    <header class="page-heading"><div><p class="eyebrow">ONBOARDING · 2 OF 4</p><h1>Review shortcuts.</h1><p class="intro">{shortcuts?.conflicts.length ?? 0} shortcut conflicts found. Review them before choosing bindings for your apps.</p></div></header>
    {#if shortcuts?.conflicts.length}<ul>{#each shortcuts.conflicts as conflict, i (`${conflict.chord}-${i}`)}<li>{conflict.chord}: {conflict.a.label} and {conflict.b.label}</li>{/each}</ul>{/if}
    <button type="button" onclick={() => onSelectTab('shortcuts')}>Open shortcut center</button>
    <div class="onboarding-footer"><button type="button" onclick={() => step = 'choose'}>Back</button><button type="button" onclick={() => step = 'tray'}>Continue / skip</button></div>
  {:else if step === 'tray'}
    <header class="page-heading"><div><p class="eyebrow">ONBOARDING · 3 OF 4</p><h1>Choose your tray and cheat sheet.</h1><p class="intro">Host app icons in one tray, choose the overlay shortcut, and set start at login.</p></div></header>
    <TraySettings active={active && step === 'tray'} busy={false} />
    <div class="onboarding-footer"><button type="button" onclick={() => step = 'shortcuts'}>Back</button><button type="button" onclick={() => step = 'review'}>Continue / skip</button></div>
  {:else if step === 'review'}
    <header class="page-heading"><div><p class="eyebrow">ARCADE TOOLS / ONBOARDING · 4 OF 4</p><h1>Review your choices.</h1><p class="intro">No download starts from this list. Use Review &amp; install to open the confirmation for an individual app.</p></div><span class="app-count">{selectedApps.length} selected</span></header>
    <div class="onboarding-app-list review-list">{#each selectedApps as app (app.id)}<div class="onboarding-app-row"><img class="app-glyph" src="/glyphs/{app.id}.svg" alt="" width="30" height="30" /><span class="onboarding-app-info"><strong>{app.name}</strong><small>{app.pitch}</small></span>{#if app.installed || app.managed}<span class="badge">Already installed</span>{#if !app.managed && app.installationClass !== 'dev'}<button type="button" onclick={() => run(() => confirmOperation(app, 'adopt'), app.name)}>Let Tools manage it</button>{/if}{:else if !app.installSupported}<span class="badge">Unavailable</span>{:else}<button type="button" class="primary-action" onclick={() => run(() => onInstall(app), app.name)}>Review &amp; install</button>{/if}</div>{/each}</div>
    <div class="onboarding-footer"><button type="button" onclick={() => step = 'choose'}>Change selection</button><button type="button" class="primary-action" onclick={finish}>Finish setup</button></div>
  {:else}
    <header class="onboarding-welcome"><p class="eyebrow">ARCADE TOOLS / READY</p><h1>You’re set.</h1><p class="intro">Your selections are ready in Apps. You can reopen this guide any time from Onboarding.</p>{#if shortcuts}<p class="setting-description">Tip: press <strong>{shortcuts.os === 'windows' ? 'Ctrl + Alt + /' : shortcuts.os === 'macos' ? 'Ctrl + Option + /' : 'Super + /'}</strong> in any app to see its shortcuts, or browse them all in <button type="button" class="text-action" onclick={() => onSelectTab('shortcuts')}>Shortcuts</button>.</p>{/if}<div class="onboarding-actions"><button type="button" onclick={() => onSelectTab('learn')}>Read getting started</button><button type="button" class="primary-action" onclick={onComplete}>Go to Home</button><button type="button" onclick={() => step = 'choose'}>Choose more apps</button></div></header>
  {/if}
</div>
