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
  };
  type View = {
    preferences: Preferences;
    apps: { id: string; name: string; excluded: boolean }[];
    loginAvailable: boolean;
  };
  let { active, busy }: { active: boolean; busy: boolean } = $props();
  let view = $state<View | null>(null);
  let saving = $state(false);
  let error = $state('');

  async function load(): Promise<void> {
    try {
      view = await invoke<View>('tray_get_settings');
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

<section class="settings-card tray-card" aria-labelledby="tray-heading">
  <div>
    <h2 id="tray-heading">Tray</h2>
    <p class="setting-description">Keep Arcade apps together in one menu. Apps restore their own icons when Tools closes or disconnects.</p>
  </div>
  {#if view}
    <div class="tray-options">
      <label><input type="checkbox" checked={view.preferences.hostIcons} disabled={busy || saving} onchange={(event) => void save({ hostIcons: event.currentTarget.checked })} /> Host Arcade app icons</label>
      <label><input type="checkbox" checked={view.preferences.showUpdateBadge} disabled={busy || saving} onchange={(event) => void save({ showUpdateBadge: event.currentTarget.checked })} /> Show update badge</label>
      <label><input type="checkbox" checked={view.preferences.shortcutsEnabled} disabled={busy || saving} onchange={(event) => void save({ shortcutsEnabled: event.currentTarget.checked })} /> Keep Tools running for shortcuts</label>
      {#if view.loginAvailable}
        <label><input type="checkbox" checked={view.preferences.startAtLogin} disabled={busy || saving} onchange={(event) => void save({ startAtLogin: event.currentTarget.checked })} /> Start Arcade Tools at login</label>
      {:else}
        <p class="setting-description">Start at login is available in installed builds.</p>
      {/if}
    </div>
    {#if view.apps.length}
      <fieldset class="tray-apps" disabled={busy || saving}>
        <legend>Keep its own icon</legend>
        {#each view.apps as app (app.id)}
          <label><input type="checkbox" checked={view.preferences.excluded.includes(app.id)} onchange={(event) => exclude(app.id, event.currentTarget.checked)} /> {app.name}</label>
        {/each}
      </fieldset>
    {/if}
  {/if}
  {#if error}<p role="alert" class="tray-error">{error}</p>{/if}
</section>

<style>
  .tray-card { display: grid; gap: 1rem; }
  .tray-options, .tray-apps { display: grid; gap: .75rem; }
  label { display: flex; align-items: center; gap: .6rem; }
  input { flex-shrink: 0; }
  .tray-apps { margin: 0; padding: 1rem; border: 1px solid var(--border); border-radius: .5rem; }
  legend { padding: 0 .4rem; }
  .tray-error { color: var(--text); }
</style>
