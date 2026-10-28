<script lang="ts">
  // The one way to set a global shortcut (STANDARD §5.1): shows the current
  // keys, records a new chord on click or Enter, checks it and applies it at
  // once. A clash asks first; Esc cancels; Backspace clears where allowed.
  import Icon from './Icon.svelte';
  import { chordFromEvent, chordParts, needsModifier, normalizeChord } from '../lib/chords';

  type Props = {
    value: string;
    label: string;
    defaultValue?: string;
    allowEmpty?: boolean;
    disabled?: boolean;
    conflictsFor?: (chord: string) => string[];
    onApply: (chord: string) => Promise<void>;
  };
  let { value, label, defaultValue, allowEmpty = false, disabled = false, conflictsFor = () => [], onApply }: Props = $props();

  let recording = $state(false);
  let busy = $state(false);
  let pending = $state<{ chord: string; owners: string[] } | null>(null);
  let hint = $state('');
  let error = $state('');
  let button: HTMLButtonElement;

  function start(): void {
    if (disabled || busy) return;
    recording = true;
    pending = null;
    hint = '';
    error = '';
  }

  async function apply(chord: string): Promise<void> {
    busy = true;
    error = '';
    try {
      await onApply(chord);
    } catch (failure) {
      error = `Couldn't set this shortcut. ${(failure as { message?: string }).message ?? String(failure)}`;
    } finally {
      busy = false;
    }
  }

  function key(event: KeyboardEvent): void {
    if (!recording) return;
    if (event.key === 'Tab') { recording = false; return; }
    event.preventDefault();
    event.stopPropagation();
    if (event.key === 'Escape') { recording = false; hint = ''; return; }
    const plain = !event.ctrlKey && !event.altKey && !event.shiftKey && !event.metaKey;
    if (event.key === 'Backspace' && plain && allowEmpty) {
      recording = false;
      void apply('');
      return;
    }
    const chord = chordFromEvent(event);
    if (!chord) return;
    if (needsModifier(chord)) {
      hint = 'Add Ctrl, Alt or Super: on its own, this key would stop working everywhere else.';
      return;
    }
    recording = false;
    hint = '';
    if (normalizeChord(chord) === normalizeChord(value)) return;
    const owners = conflictsFor(chord);
    if (owners.length) {
      pending = { chord, owners };
      return;
    }
    void apply(chord);
  }

  function useAnyway(): void {
    const chord = pending?.chord;
    pending = null;
    if (chord) void apply(chord);
  }

  function chooseAnother(): void {
    pending = null;
    start();
    button?.focus();
  }
</script>

<div class="recorder">
  <button
    bind:this={button}
    type="button"
    class="shortcut-recorder"
    class:recording
    aria-label="{label}: {value ? chordParts(value).join(' ') : 'no shortcut'}. {recording ? 'Press the new keys, or Escape to cancel.' : 'Press Enter to change.'}"
    {disabled}
    onclick={start}
    onkeydown={key}
    onblur={() => { recording = false; }}
  >
    {#if recording}
      Press keys…
    {:else if value}
      <span class="key-chip-list">{#each chordParts(value) as part, i (`${part}-${i}`)}<kbd class="key-chip">{part}</kbd>{/each}</span>
    {:else}
      <span class="faint">No shortcut</span>
    {/if}
  </button>
  {#if defaultValue !== undefined && normalizeChord(value) !== normalizeChord(defaultValue)}
    <button type="button" class="btn ghost sm" disabled={disabled || busy} onclick={() => void apply(defaultValue)}>Reset to default</button>
  {/if}
</div>
{#if pending}
  <div class="notice warn recorder-notice" role="alert">
    <Icon name="alert" />
    <span class="row-main">{chordParts(pending.chord).join(' ')} is used by {pending.owners.join(', ')}.</span>
    <button type="button" class="btn sm" onclick={useAnyway}>Use anyway</button>
    <button type="button" class="btn sm ghost" onclick={chooseAnother}>Choose another</button>
  </div>
{/if}
{#if hint}<p class="status-warning recorder-hint" role="alert">{hint}</p>{/if}
{#if error}<p class="error-text" role="alert">{error}</p>{/if}

<style>
  .recorder { display: inline-flex; flex-wrap: wrap; align-items: center; gap: var(--sp-6); }
  .recorder-notice, .recorder-hint { width: 100%; margin-top: var(--sp-8); }
</style>
