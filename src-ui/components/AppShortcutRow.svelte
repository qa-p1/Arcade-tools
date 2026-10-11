<script lang="ts">
  import KeyChips from './KeyChips.svelte';
  import type { AppShortcut } from '../lib/shortcuts';

  type Props = { shortcut: AppShortcut; os: string };
  let { shortcut, os }: Props = $props();
</script>

<li class="shortcut-row">
  <div class="shortcut-copy">
    <h3>{shortcut.title}</h3>
    {#if shortcut.description}<p>{shortcut.description}</p>{/if}
  </div>
  <KeyChips keys={shortcut.display} {os} />
</li>
