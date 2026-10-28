<script lang="ts">
  // Rows of the tray, shortcut overlay and start-at-login preferences, for a
  // parent `.list`: Settings puts each part in its own section, onboarding
  // shows them together.
  import { untrack } from 'svelte';
  import { ui } from '../lib/state.svelte';
  import { keepOwnIcon, loadTray, saveTray, tray, watchTray } from '../lib/traySettings.svelte';
  import { normalizeChord } from '../lib/chords';
  import ShortcutRecorder from './ShortcutRecorder.svelte';
  import Icon from './Icon.svelte';

  type Part = 'login' | 'overlay' | 'tray';
  let { active, busy, parts }: { active: boolean; busy: boolean; parts: Part[] } = $props();
  const view = $derived(tray.view);
  const locked = $derived(busy || tray.saving);

  $effect(() => {
    if (active) untrack(() => { watchTray(() => active); void loadTray(); });
  });

  // Other Arcade apps and the window manager, from the shortcut list.
  function conflictsFor(chord: string): string[] {
    const index = ui.shortcutsIndex;
    if (!index) return [];
    const wanted = normalizeChord(chord);
    const arcade = index.arcade
      .filter((other) => other.app !== 'arcade.tools' && other.keys.some((key) => normalizeChord(key) === wanted))
      .map((other) => `${other.name} (${other.title})`);
    const wmName = index.wm.name && index.wm.name !== 'none' ? index.wm.name : 'your desktop';
    const wm = index.wm.groups.flatMap((group) => group.shortcuts)
      .filter((binding) => normalizeChord(binding.chord) === wanted)
      .map((binding) => `${wmName} (${binding.label})`);
    return [...arcade, ...wm];
  }

  async function applyShortcut(chord: string): Promise<void> {
    await saveTray({ shortcut: chord });
    if (tray.error) throw new Error(tray.error);
  }
</script>

{#if !view && tray.failed}
  <div class="row">
    <Icon name="alert" />
    <div class="row-main"><div class="row-title">Couldn't load these settings</div><div class="row-sub">{tray.error}</div></div>
    <button type="button" class="btn sm" onclick={() => void loadTray()}>Retry</button>
  </div>
{:else if !view}
  <div class="skeleton-row"><div class="row-main"><div class="skeleton line skel"></div></div></div>
{:else}
  {#if parts.includes('login')}
    {#if view.loginAvailable}
      <label class="row">
        <span class="row-main"><span class="row-title">Start at login</span><span class="row-sub">Arcade Tools starts in the tray, without a window.</span></span>
        <input class="switch" type="checkbox" aria-label="Start at login" checked={view.preferences.startAtLogin} disabled={locked} onchange={(event) => void saveTray({ startAtLogin: event.currentTarget.checked })} />
      </label>
    {:else}
      <div class="row">
        <div class="row-main"><div class="row-title">Start at login</div><div class="row-sub">Available once Arcade Tools is installed.</div></div>
      </div>
    {/if}
  {/if}
  {#if parts.includes('overlay')}
    {#if view.shortcutStatus.available}
      <label class="row">
        <span class="row-main"><span class="row-title">Shortcut overlay</span><span class="row-sub">A sheet of the shortcuts for the app you're using.</span></span>
        <input class="switch" type="checkbox" aria-label="Shortcut overlay" checked={view.preferences.shortcutsEnabled} disabled={locked} onchange={(event) => void saveTray({ shortcutsEnabled: event.currentTarget.checked })} />
      </label>
      {#if view.preferences.shortcutsEnabled}
        <div class="row overlay-shortcut">
          <div class="row-main"><div class="row-title" id="overlay-shortcut-label">Overlay shortcut</div></div>
          <ShortcutRecorder value={view.preferences.shortcut} label="Overlay shortcut" defaultValue="Super+Slash" disabled={locked} {conflictsFor} onApply={applyShortcut} />
        </div>
        {#if view.shortcutStatus.hint || view.shortcutStatus.error}
          <div class="row-extra flush">
            {#if view.shortcutStatus.hint}<p class="row-sub shortcut-hint">{view.shortcutStatus.hint}</p>{/if}
            {#if view.shortcutStatus.error}<p role="alert" class="error-text">Couldn't register this shortcut. Another app or the desktop may be using it; choose another. ({view.shortcutStatus.error})</p>{/if}
          </div>
        {/if}
      {/if}
    {:else}
      <div class="row">
        <div class="row-main"><div class="row-title">Shortcut overlay</div><div class="row-sub">Comes with installed builds of Arcade Tools.</div></div>
      </div>
    {/if}
  {/if}
  {#if parts.includes('tray')}
    <label class="row">
      <span class="row-main"><span class="row-title">Host Arcade app icons</span><span class="row-sub">Keep the apps together in one tray menu. They get their own icons back when Tools closes.</span></span>
      <input class="switch" type="checkbox" aria-label="Host Arcade app icons" checked={view.preferences.hostIcons} disabled={locked} onchange={(event) => void saveTray({ hostIcons: event.currentTarget.checked })} />
    </label>
    <label class="row">
      <span class="row-main"><span class="row-title">Show update badge</span><span class="row-sub">A dot on the tray icon when updates are ready.</span></span>
      <input class="switch" type="checkbox" aria-label="Show update badge" checked={view.preferences.showUpdateBadge} disabled={locked} onchange={(event) => void saveTray({ showUpdateBadge: event.currentTarget.checked })} />
    </label>
    {#if view.apps.length}
      <fieldset class="tray-apps" disabled={locked}>
        <legend class="row-title">Apps that keep their own icon</legend>
        <div class="app-checks">
          {#each view.apps as app (app.id)}
            <label class="check-row"><input type="checkbox" aria-label="Keep its own icon: {app.name}" checked={view.preferences.excluded.includes(app.id)} onchange={(event) => keepOwnIcon(app.id, event.currentTarget.checked)} /> {app.name}</label>
          {/each}
        </div>
      </fieldset>
    {/if}
  {/if}
  {#if tray.error}<p role="alert" class="error-text tray-error">Couldn't save this setting. {tray.error}</p>{/if}
{/if}

<style>
  label.row .row-title, label.row .row-sub { display: block; }
  .overlay-shortcut { flex-wrap: wrap; }
  .row-extra.flush { padding: 0 14px 12px; }
  .shortcut-hint { white-space: pre-wrap; }
  .tray-apps { margin: 0; padding: 12px 14px 14px; border: 0; border-top: 1px solid var(--line); }
  .tray-apps legend { float: left; width: 100%; margin-bottom: 10px; padding: 0; }
  .app-checks { display: grid; clear: both; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px 16px; font-size: var(--fs-control); }
  .tray-error { padding: 0 14px 12px; }
  .skel { width: 40%; }
</style>
