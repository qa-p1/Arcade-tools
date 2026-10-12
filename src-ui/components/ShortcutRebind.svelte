<script lang="ts">
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
  async function save(): Promise<void> {
    const answer = await askForConfirmation({ title: `Change ${shortcut.name} shortcut?`, copy: `${shortcut.title}: ${shortcut.keys.join(' or ')} → ${draft}. The app saves this global shortcut.`, action: 'Apply', destructive: false, kind: null, folders: [] });
    if (!answer.confirmed) return;
    busy = true;
    try {
      const result = await api.setAppShortcut(shortcut.app, shortcut.id, draft);
      message = `${result.applied ? 'Applied' : 'Registration needs your attention'} · ${result.via}${result.hint ? `: ${result.hint}` : ''}`;
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
    <small>Tools checks other Arcade globals and {index.wm.name} bindings before applying.</small>
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
  input { margin-left:.4rem; max-width:15rem; }
</style>
