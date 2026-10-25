<script lang="ts">
  type Props = { keys: string[]; os: string };
  let { keys, os }: Props = $props();
  // Each entry is one chord; "Ctrl+Shift+K" shows as three keys.
  const chords = $derived(keys.map((chord) => chord.split(/\s*\+\s*(?=.)/).filter(Boolean)));
</script>

<span class="key-chip-list" data-os={os} aria-label={`Shortcut keys: ${keys.join(', ')}`}>
  {#each chords as parts, index (`${keys[index]}-${index}`)}
    {#if index > 0}<span class="key-or">or</span>{/if}
    {#each parts as part, partIndex (`${part}-${partIndex}`)}<kbd class="key-chip">{part}</kbd>{/each}
  {/each}
</span>
