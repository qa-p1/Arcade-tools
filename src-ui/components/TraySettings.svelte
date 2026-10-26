<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { ui } from '../lib/state.svelte';

  type Preferences = {
    hostIcons: boolean;
    excluded: string[];
    showUpdateBadge: boolean;
    startAtLogin: boolean;
    shortcutsEnabled: boolean;
    shortcut: string;
  };
  type View = {
    preferences: Preferences;
    apps: { id: string; name: string; excluded: boolean }[];
    loginAvailable: boolean;
    shortcutStatus: { accelerator: string; available: boolean; via: string | null; hint: string | null; error: string | null };
  };
  let { active, busy }: { active: boolean; busy: boolean } = $props();
  let view = $state<View | null>(null);
  let saving = $state(false);
  let error = $state('');
  let shortcut = $state('');

  async function load(): Promise<void> {
    try {
      view = await invoke<View>('tray_get_settings');
      shortcut = view.preferences.shortcut;
      if (ui.testing) ui.shortcutsRevision += 1;
      error = '';
    } catch (failure) {
      error = (failure as { message?: string }).message ?? String(failure);
    }
  }

  async function save(change: Partial<Preferences>): Promise<void> {
    if (!view || saving) return;
    saving = true;
    error = '';
    try {
      const preferences = await invoke<Preferences>('tray_set_settings', {
        preferences: { ...view.preferences, ...change },
      });
      view = { ...view, preferences };
      if (ui.testing) ui.shortcutsRevision += 1;
    } catch (failure) {
      error = (failure as { message?: string }).message ?? String(failure);
    } finally {
      saving = false;
    }
  }

  function exclude(id: string, own: boolean): void {
    if (!view) return;
    const excluded = view.preferences.excluded.filter((app) => app !== id);
    if (own) excluded.push(id);
    void save({ excluded });
  }

  function navigate(page: string | null): void {
    if (page === 'settings' || page === 'apps') ui.tab = page;
  }

  $effect(() => {
    if (active) untrack(() => { void load(); });
  });

  onMount(() => {
    let disposed = false;
    const cleanup: UnlistenFn[] = [];
    async function subscribe(): Promise<void> {
      const listeners = await Promise.all([
        listen<string>('tray-navigation', ({ payload }) => navigate(payload)),
        listen('tray-settings-changed', () => { if (active) void load(); }),
        listen('overlay-shortcut-changed', () => { if (active) void load(); }),
        listen('registry-changed', () => { if (active) void load(); }),
      ]);
      if (disposed) listeners.forEach((stop) => stop());
      else cleanup.push(...listeners);
      navigate(await invoke<string | null>('tray_ui_intent'));
    }
    void subscribe().catch(() => { /* Startup can precede the resident service. */ });
    return () => { disposed = true; cleanup.forEach((stop) => stop()); };
  });
</script>

<section class="section tray-card" aria-labelledby="tray-heading">
  <div class="section-head"><h2 id="tray-heading">Tray and shortcut overlay</h2></div>
  {#if view}
    <div class="list">
      <label class="row">
        <span class="row-main"><span class="row-title">Host Arcade app icons</span><span class="row-sub">Keep the apps together in one tray menu. They get their own icons back when Tools closes.</span></span>
        <input class="switch" type="checkbox" aria-label="Host Arcade app icons" checked={view.preferences.hostIcons} disabled={busy || saving} onchange={(event) => void save({ hostIcons: event.currentTarget.checked })} />
      </label>
      <label class="row">
        <span class="row-main"><span class="row-title">Show update badge</span></span>
        <input class="switch" type="checkbox" aria-label="Show update badge" checked={view.preferences.showUpdateBadge} disabled={busy || saving} onchange={(event) => void save({ showUpdateBadge: event.currentTarget.checked })} />
      </label>
      {#if view.shortcutStatus.available}
        <label class="row">
          <span class="row-main"><span class="row-title">Shortcut overlay</span><span class="row-sub">A sheet of the shortcuts for the app you're using.</span></span>
          <input class="switch" type="checkbox" aria-label="Enable shortcut overlay" checked={view.preferences.shortcutsEnabled} disabled={busy || saving} onchange={(event) => void save({ shortcutsEnabled: event.currentTarget.checked })} />
        </label>
        {#if view.preferences.shortcutsEnabled}
          <div class="row">
            <label class="row-main row-title" for="overlay-shortcut">Overlay shortcut</label>
            <form class="shortcut-form" onsubmit={(event) => { event.preventDefault(); void save({ shortcut }); }}>
              <input id="overlay-shortcut" class="input" bind:value={shortcut} disabled={busy || saving} autocomplete="off" spellcheck={false} />
              <button class="btn sm" type="submit" disabled={busy || saving || shortcut === view.preferences.shortcut}>Apply</button>
            </form>
          </div>
          {#if view.shortcutStatus.hint || view.shortcutStatus.error}
            <div class="row-extra flush">
              {#if view.shortcutStatus.hint}<p class="row-sub shortcut-hint">{view.shortcutStatus.hint}</p>{/if}
              {#if view.shortcutStatus.error}<p role="alert" class="error-text">Could not register the shortcut: {view.shortcutStatus.error}</p>{/if}
            </div>
          {/if}
        {/if}
      {/if}
      {#if view.loginAvailable}
        <label class="row">
          <span class="row-main"><span class="row-title">Start Arcade Tools at login</span></span>
          <input class="switch" type="checkbox" aria-label="Start Arcade Tools at login" checked={view.preferences.startAtLogin} disabled={busy || saving} onchange={(event) => void save({ startAtLogin: event.currentTarget.checked })} />
        </label>
      {/if}
      {#if view.apps.length}
        <fieldset class="tray-apps" disabled={busy || saving}>
          <legend class="row-title">Apps that keep their own icon</legend>
          <div class="app-checks">
            {#each view.apps as app (app.id)}
              <label class="check-row"><input type="checkbox" aria-label="Keep its own icon: {app.name}" checked={view.preferences.excluded.includes(app.id)} onchange={(event) => exclude(app.id, event.currentTarget.checked)} /> {app.name}</label>
            {/each}
          </div>
        </fieldset>
      {/if}
    </div>
    {#if !view.shortcutStatus.available || !view.loginAvailable}
      <p class="section-note">{!view.shortcutStatus.available ? 'The shortcut overlay comes with installed builds of Tools. ' : ''}{!view.loginAvailable ? 'Start at login is available in installed builds.' : ''}</p>
    {/if}
  {:else}
    <div class="list"><div class="skeleton-row"><div class="row-main"><div class="skeleton line skel"></div></div></div></div>
  {/if}
  {#if error}<p role="alert" class="error-text tray-error">{error}</p>{/if}
</section>

<style>
  label.row .row-title, label.row .row-sub { display: block; }
  .shortcut-form { display: flex; gap: 8px; }
  .shortcut-form .input { width: 170px; }
  .row-extra.flush { padding: 0 14px 12px; }
  .shortcut-hint { white-space: pre-wrap; }
  .tray-apps { margin: 0; padding: 12px 14px 14px; border: 0; border-top: 1px solid var(--line); }
  .tray-apps legend { float: left; width: 100%; margin-bottom: 10px; padding: 0; }
  .app-checks { display: grid; clear: both; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px 16px; font-size: 12.5px; }
  .tray-error { margin-top: 8px; }
  .skel { width: 40%; }
</style>
