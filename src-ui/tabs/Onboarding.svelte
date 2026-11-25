<script lang="ts">
  import AppMark from '../components/AppMark.svelte';
  import TraySettings from '../components/TraySettings.svelte';
  import Icon from '../components/Icon.svelte';
  import { asFailure, confirmOperation, installBundle, launchApp } from '../lib/controller';
  import { askForConfirmation, ui } from '../lib/state.svelte';
  import KeyChips from '../components/KeyChips.svelte';
  import { api, type AppView, type Bundle } from '../lib/api';
  import { setTabEnabled, type ShellTab } from '../lib/tabs.svelte';
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
  let step = $state<'welcome' | 'choose' | 'setup' | 'review' | 'done'>('welcome');
  let selected = $state<Set<string>>(new Set());
  let loading = $state(false);
  let loaded = $state(false);
  let error = $state<string | null>(null);
  const selectedApps = $derived(apps.filter((app) => selected.has(app.id)));
  const toInstall = $derived(selectedApps.filter((app) => !app.installed && !app.managed && app.installSupported));
  // The apps set up during this run, for "Try your apps".
  let chosen = $state<string[]>([]);
  const tryApps = $derived(apps.filter((app) => chosen.includes(app.id) && (app.installed || app.managed)));
  let installing = $state(false);
  // At most three steps (STANDARD §5.1): apps, then shortcuts and tray, then review.
  const steps = ['choose', 'setup', 'review'] as const;
  const stepIndex = $derived(steps.indexOf(step as typeof steps[number]));

  async function loadBundles(): Promise<void> {
    if (loaded || loading) return;
    loading = true;
    error = null;
    try { bundles = await api.listBundles(); loaded = true; }
    catch (problem) { error = asFailure(problem).message; }
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

  /** The app's own global shortcut (the first one it registers), shown so people learn the key. */
  function shortcutOf(app: AppView): string[] {
    return shortcuts?.arcade.find((shortcut) => shortcut.app === app.id)?.display ?? [];
  }

  /** One confirmation for every app picked, then they install one after another. */
  async function installAll(): Promise<void> {
    const ids = toInstall.map((app) => app.id);
    if (!ids.length) { finish(); return; }
    // The "every app" bundle allows any catalog app; without bundles each app is confirmed on its own.
    const everything = bundles.find((bundle) => bundle.apps === '*');
    if (!everything) {
      for (const app of toInstall) await onInstall(app);
      finish();
      return;
    }
    const choice = await askForConfirmation({
      title: `Install ${ids.length} ${ids.length === 1 ? 'app' : 'apps'}?`,
      copy: `${toInstall.map((app) => `• ${app.name}`).join('\n')}\nThe stable release of each installs for your user account, one after another, and starts in the background.`,
      action: 'Install',
      destructive: false,
      kind: null,
      folders: [],
    });
    if (!choice.confirmed) return;
    installing = true;
    try {
      await installBundle(everything.id, ids, ids.length === 1 ? `Installing ${toInstall[0].name}` : `Installing ${ids.length} apps`);
      ui.shortcutsIndex = await api.shortcutsIndex().catch(() => ui.shortcutsIndex);
    } finally {
      installing = false;
    }
    finish();
  }

  function finish(): void {
    chosen = [...selected];
    try { localStorage.setItem('arcade-tools-onboarding-complete', 'yes'); } catch { /* finish for this session */ }
    // The preference is the durable record; the local flag only covers a failed write.
    run(() => api.completeOnboarding());
    setTabEnabled('onboarding', false);
    step = 'done';
  }

  $effect(() => { if (active) void loadBundles(); });
</script>

{#snippet progress()}
  <div class="steps" aria-label="Step {stepIndex + 1} of {steps.length}">{#each steps as item, i (item)}<span class:done={i <= stepIndex}></span>{/each}</div>
{/snippet}

<div id="panel-onboarding" class="tab-panel" role="tabpanel" aria-labelledby="tab-onboarding" tabindex="0" hidden={!active}>
  {#if step === 'welcome'}
    <div class="welcome">
      <AppMark id={'arcade.tools'} size="lg" />
      <h1>Welcome to Arcade Tools</h1>
      <p class="lede">The optional home for the Arcade apps.</p>
      <div class="btn-group"><button type="button" class="btn primary" onclick={() => step = 'choose'}>Choose apps</button><button type="button" class="btn ghost" onclick={finish}>Skip setup</button></div>
    </div>
  {:else if step === 'choose'}
    {@render progress()}
    <header class="page-head">
      <div><h1>What would you like to try?</h1><p class="sub">Pick a bundle or single apps. Nothing installs until you confirm.</p></div>
      <button type="button" class="btn ghost" onclick={finish}>Skip setup</button>
    </header>
    {#if error}<div class="notice warn" role="alert"><Icon name="alert" /><span class="row-main">Couldn't load the bundles. {error} You can still pick single apps below.</span><button type="button" class="btn sm" onclick={() => { loaded = false; void loadBundles(); }}>Retry</button></div>{/if}
    {#if bundles.length}
      <section class="section">
        <div class="section-head"><h2>Bundles</h2></div>
        <div class="bundle-list">{#each bundles as bundle (bundle.id)}<button class="card bundle-card" type="button" onclick={() => chooseBundle(bundle)}><strong>{bundle.title}</strong><span>{bundle.description}</span><small>{bundleIds(bundle).length} apps</small></button>{/each}</div>
      </section>
    {/if}
    <section class="section">
      <div class="section-head"><h2>Apps</h2><span class="meta">{selected.size} selected</span></div>
      <div class="list">{#each apps as app (app.id)}<label class="row"><input type="checkbox" checked={selected.has(app.id)} onchange={(event) => toggle(app.id, event.currentTarget.checked)} /><AppMark id={app.id} size="sm" /><span class="row-main"><span class="row-title">{app.name}</span><span class="row-sub">{app.tagline}</span></span>{#if app.installed || app.managed}<span class="badge">Installed</span>{:else if !app.installSupported}<span class="badge">Not available</span>{/if}</label>{/each}</div>
    </section>
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'welcome'}>Back</button><button type="button" class="btn primary" disabled={selected.size === 0} onclick={() => step = 'setup'}>Continue with {selected.size}</button></div>
  {:else if step === 'setup'}
    {@render progress()}
    <header class="page-head">
      <div><h1>Shortcuts and tray</h1><p class="sub">{shortcuts?.conflicts.length ? `${shortcuts.conflicts.length} shortcuts clash. You can sort them out now or later.` : 'No shortcuts clash on this computer.'}</p></div>
      <button type="button" class="btn ghost" onclick={finish}>Skip setup</button>
    </header>
    {#if shortcuts?.conflicts.length}<ul class="list plain">{#each shortcuts.conflicts as conflict, i (`${conflict.chord}-${i}`)}<li class="row"><span class="dot warn"></span><span class="row-main">{conflict.a.label} <span class="faint">and</span> {conflict.b.label}</span><kbd class="key-chip">{conflict.chord}</kbd></li>{/each}</ul>{/if}
    <section class="section">
      <div class="section-head"><h2>Tray and shortcut overlay</h2><span class="meta">One tray menu for every app, and a shortcut sheet on demand.</span></div>
      <div class="list"><TraySettings active={active && step === 'setup'} busy={false} parts={['tray', 'overlay', 'login']} /></div>
    </section>
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'choose'}>Back</button><span class="spacer"></span><button type="button" class="btn" onclick={() => onSelectTab('shortcuts')}>Open Shortcuts</button><button type="button" class="btn primary" onclick={() => step = 'review'}>Continue</button></div>
  {:else if step === 'review'}
    {@render progress()}
    <header class="page-head"><div><h1>Review and install</h1><p class="sub">{toInstall.length ? 'One confirmation installs them all.' : 'Everything you picked is already here.'}</p></div><button type="button" class="btn ghost" onclick={finish}>Skip setup</button></header>
    <div class="list">{#each selectedApps as app (app.id)}<div class="row"><AppMark id={app.id} size="sm" /><span class="row-main"><span class="row-title">{app.name}</span><span class="row-sub">{app.tagline}</span></span>{#if app.installed || app.managed}{#if !app.managed && app.installationClass !== 'dev'}<button type="button" class="btn sm" onclick={() => run(() => confirmOperation(app, 'adopt'), app.name)}>Let Tools manage it</button>{:else}<span class="badge">Installed</span>{/if}{:else if !app.installSupported}<span class="badge">Unavailable</span>{:else}<span class="badge">To install</span>{/if}</div>{/each}</div>
    <div class="footer"><button type="button" class="btn ghost" onclick={() => step = 'setup'}>Back</button>{#if toInstall.length}<button type="button" class="btn primary" disabled={installing || ui.busy} onclick={() => run(installAll)}>{installing ? 'Installing…' : `Install ${toInstall.length} ${toInstall.length === 1 ? 'app' : 'apps'}`}</button>{:else}<button type="button" class="btn primary" onclick={finish}>Finish setup</button>{/if}</div>
  {:else}
    <div class="welcome">
      <span class="glyph-tile lg done"><Icon name="check" size={24} /></span>
      <h1>You're all set</h1>
      <p class="lede">{tryApps.length ? 'Try your apps. Each one has a shortcut that works from any app.' : 'Your apps are in Apps. You can set them up again from Settings › General.'}</p>
      {#if tryApps.length}
        <div class="list try-list">{#each tryApps as app (app.id)}{@const keys = shortcutOf(app)}<div class="row"><AppMark id={app.id} /><span class="row-main"><span class="row-title">{app.name}</span><span class="row-sub">{app.tagline}</span></span>{#if keys.length}<KeyChips keys={keys} os={shortcuts?.os ?? ""} />{/if}<button type="button" class="btn sm" onclick={() => run(() => launchApp(app, 'foreground'), app.name)}>Try it</button></div>{/each}</div>
      {/if}
      {#if shortcuts}<p class="tip muted">Press <kbd class="key-chip">{shortcuts.os === 'windows' ? 'Ctrl+Alt+/' : shortcuts.os === 'macos' ? 'Ctrl+Option+/' : 'Super+/'}</kbd> in any app to see its shortcuts.</p>{/if}
      <div class="btn-group"><button type="button" class="btn primary" onclick={onComplete}>Go to Home</button><button type="button" class="btn ghost" onclick={() => onSelectTab('learn')}>Read the guide</button><button type="button" class="btn ghost" onclick={() => step = 'choose'}>Choose more apps</button></div>
    </div>
  {/if}
</div>

<style>
  .welcome { display: grid; justify-items: start; gap: 12px; max-width: 640px; padding: 48px 0 24px; }
  .welcome h1 { margin-top: 8px; font-size: var(--fs-display); line-height: var(--lh-display); font-weight: var(--fw-display); }
  .lede { color: var(--muted); font-size: var(--fs-heading); line-height: 1.6; }
  .welcome .btn-group { margin-top: 12px; }
  .try-list { width: 100%; margin-top: 8px; }
  .done { color: var(--accent); }
  .tip { display: flex; align-items: center; gap: 6px; }
  .steps { display: flex; gap: 6px; margin-bottom: 20px; }
  .steps span { width: 28px; height: 3px; border-radius: 99px; background: var(--line-strong); }
  .steps span.done { background: var(--accent); }
  .bundle-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 10px; }
  .bundle-card { display: grid; grid-template-columns: minmax(0, 1fr); align-content: start; gap: 4px; margin: 0; color: var(--text); text-align: left; white-space: normal; overflow-wrap: anywhere; cursor: pointer; }
  .bundle-card + .bundle-card { margin-top: 0; }
  .bundle-card:hover { background: var(--sunken); }
  .bundle-card span { color: var(--muted); font-size: var(--fs-control); }
  .bundle-card small { color: var(--muted); font-size: var(--fs-small); }
  label.row .row-title, label.row .row-sub { display: block; }
  .plain { margin: 0; padding: 0; list-style: none; }
  .footer { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: 24px; }
  .spacer { flex: 1; }
</style>
