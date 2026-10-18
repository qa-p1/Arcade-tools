<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { api } from '../lib/api';
  import type { ArcadeShortcut, ShortcutsIndex } from '../lib/shortcuts';
  import { askForConfirmation } from '../lib/state.svelte';
  let { shortcut, index }: { shortcut: ArcadeShortcut; index: ShortcutsIndex } = $props();
  let editing = $state(false);
  let draft = $state('');
  let busy = $state(false);
  let message = $state('');
  function record(event: KeyboardEvent): void {
    if (event.key === 'Tab' || event.key === 'Escape') return;
    event.preventDefault();
    if (['Control','Alt','Shift','Meta'].includes(event.key)) return;
    const key = event.code.replace(/^Key/, '').replace(/^Digit/, '').replace(/^Arrow/, '');
    draft = [...(event.ctrlKey ? ['Ctrl'] : []), ...(event.altKey ? ['Alt'] : []), ...(event.shiftKey ? ['Shift'] : []), ...(event.metaKey ? ['Super'] : []), key].join('+');
  }
  const aliases: Record<string, string> = { control: 'ctrl', meta: 'super', win: 'super', mod4: 'super', logo: 'super', cmd: 'super', mod1: 'alt', option: 'alt' };
  /** Comparable form of a chord: lower-case, modifiers sorted, aliases folded. */
  function normalize(chord: string): string {
    const parts = chord.toLowerCase().replace(/\s+/g, '').split('+').filter(Boolean).map((part) => aliases[part] ?? part);
    const key = parts.pop() ?? '';
    return [...new Set(parts)].sort().concat(key).join('+');
  }
  // Checked while typing, so a clash shows before anything is confirmed.
  const conflicts = $derived.by(() => {
    const wanted = normalize(draft);
    if (!draft.trim() || shortcut.keys.some((key) => normalize(key) === wanted)) return [];
    const arcade = index.arcade
      .filter((other) => !(other.app === shortcut.app && other.id === shortcut.id) && other.keys.some((key) => normalize(key) === wanted))
      .map((other) => `${other.name}: ${other.title}`);
    const wm = index.wm.groups.flatMap((group) => group.shortcuts)
      .filter((binding) => normalize(binding.chord) === wanted)
      .map((binding) => `${wmName}: ${binding.label}`);
    return [...arcade, ...wm];
  });
  const wmName = $derived(index.wm.name && index.wm.name !== 'none' ? index.wm.name : 'your desktop');
  async function save(): Promise<void> {
    const clash = conflicts.length ? `\nAlready used by ${conflicts.join(', ')}.` : '';
    const answer = await askForConfirmation({ title: `Change ${shortcut.name} shortcut?`, copy: `${shortcut.title}: ${shortcut.keys.join(' or ')} → ${draft}. The app saves this global shortcut.${clash}`, action: conflicts.length ? 'Apply anyway' : 'Apply', destructive: conflicts.length > 0, kind: null, folders: [] });
    if (!answer.confirmed) return;
    busy = true;
    try {
      if (shortcut.app === 'arcade.tools') {
        // Tools' own overlay shortcut is a tray preference (also in Settings).
        const view = await invoke<{ preferences: Record<string, unknown> }>('tray_get_settings');
        await invoke('tray_set_settings', { preferences: { ...view.preferences, shortcut: draft } });
        message = 'Applied';
      } else {
        const result = await api.setAppShortcut(shortcut.app, shortcut.id, draft);
        message = `${result.applied ? 'Applied' : 'Registration needs your attention'} · ${result.via}${result.hint ? `: ${result.hint}` : ''}`;
      }
      editing = false;
    } catch (error) { message = (error as { message?: string }).message ?? String(error); }
    finally { busy = false; }
  }
</script>
<div class="rebind-control">
  {#if editing}
    <label>Record or enter a shortcut for {shortcut.title}<input aria-label="New shortcut for {shortcut.title}" bind:value={draft} onkeydown={record} /></label>
    <button type="button" disabled={busy || !draft.trim()} onclick={() => void save()}>Apply</button>
    <button type="button" disabled={busy} onclick={() => editing = false}>Cancel</button>
    {#if conflicts.length}<p class="status-warning" role="alert">Already used by {conflicts.join(', ')}.</p>{/if}
    <small>{index.wm.name && index.wm.name !== 'none' ? `Checked against other Arcade shortcuts and ${index.wm.name} bindings.` : 'Checked against other Arcade shortcuts; no window manager configuration was found to check.'}</small>
  {:else if shortcut.rebindable}
    <button type="button" class="small-button" onclick={() => { draft = shortcut.keys[0] ?? ''; editing = true; message = ''; }}>Change shortcut</button>
  {:else}
    <button type="button" class="small-button" onclick={() => void api.openAppSettings(shortcut.app).catch((error) => message = error.message ?? String(error))}>Open app settings</button>
  {/if}
  {#if message}<p role="status">{message}</p>{/if}
</div>
<style>
  .rebind-control { display:flex; flex-wrap:wrap; gap:.4rem; align-items:center; }
  label, small, p { width:100%; }
  input { margin-left:.4rem; max-width:15rem; min-width:0; }
  .rebind-control { min-width:0; }
  .rebind-control button { white-space:nowrap; }
</style>
