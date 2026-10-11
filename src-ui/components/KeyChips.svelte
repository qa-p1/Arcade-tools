<script lang="ts">
  type Props = { keys: string[]; os: string };
  let { keys, os }: Props = $props();
</script>

<span class="key-chip-list" data-os={os} aria-label={`Shortcut keys: ${keys.join(', ')}`}>
  {#each keys as key, index (`${key}-${index}`)}
    <kbd class="key-chip">{key}</kbd>
  {/each}
</span>
