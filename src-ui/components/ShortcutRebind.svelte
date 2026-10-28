<script lang="ts">
  import { api } from '../lib/api';
  import { normalizeChord } from '../lib/chords';
  import type { ArcadeShortcut, ShortcutsIndex } from '../lib/shortcuts';
  import { saveTray, tray, loadTray } from '../lib/traySettings.svelte';
  import ShortcutRecorder from './ShortcutRecorder.svelte';
  let { shortcut, index }: { shortcut: ArcadeShortcut; index: ShortcutsIndex } = $props();
  let current = $state('');
  let message = $state('');
  $effect(() => { current = shortcut.keys[0] ?? ''; });
  const wmName = $derived(index.wm.name && index.wm.name !== 'none' ? index.wm.name.charAt(0).toUpperCase() + index.wm.name.slice(1) : 'your desktop');

  // Other Arcade apps' shortcuts and the window manager's binds.
  function conflictsFor(chord: string): string[] {
    const wanted = normalizeChord(chord);
    const arcade = index.arcade
      .filter((other) => !(other.app === shortcut.app && other.id === shortcut.id) && other.keys.some((key) => normalizeChord(key) === wanted))
      .map((other) => `${other.name} (${other.title})`);
    const wm = index.wm.groups.flatMap((group) => group.shortcuts)
      .filter((binding) => normalizeChord(binding.chord) === wanted)
      .map((binding) => `${wmName} (${binding.label})`);
    return [...arcade, ...wm];
  }

  async function apply(chord: string): Promise<void> {
    message = '';
    if (shortcut.app === 'arcade.tools') {
      // Tools' own overlay shortcut is a tray preference (also in Settings).
      if (!tray.view) await loadTray();
      await saveTray({ shortcut: chord });
      if (tray.error) throw new Error(tray.error);
    } else {
      const result = await api.setAppShortcut(shortcut.app, shortcut.id, chord);
      if (!result.applied) message = `${shortcut.name} saved the shortcut, but ${result.via} needs a step from you${result.hint ? `: ${result.hint}` : '.'}`;
    }
    current = chord;
  }
</script>
<div class="rebind-control">
  {#if shortcut.rebindable}
    <ShortcutRecorder value={current} label="Shortcut for {shortcut.title}" {conflictsFor} onApply={apply} />
  {:else}
    <button type="button" class="btn sm ghost" onclick={() => void api.openAppSettings(shortcut.app).catch((error) => message = `Couldn't open ${shortcut.name}'s settings. ${error.message ?? String(error)}`)}>Open app settings</button>
  {/if}
  {#if message}<p class="rebind-message" role="status">{message}</p>{/if}
</div>
<style>
  .rebind-control { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: var(--sp-6); min-width: 0; }
  p { width: 100%; margin: 0; text-align: right; }
  .rebind-message { color: var(--muted); font-size: var(--fs-small); }
</style>
