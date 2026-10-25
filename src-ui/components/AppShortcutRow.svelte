<script lang="ts">
  import KeyChips from './KeyChips.svelte';
  import type { AppShortcut } from '../lib/shortcuts';

  type Props = { shortcut: AppShortcut; os: string };
  let { shortcut, os }: Props = $props();
</script>

<li class="row shortcut-row">
  <div class="row-main">
    <div class="row-title">{shortcut.title}</div>
    {#if shortcut.description && shortcut.description !== shortcut.title}<div class="row-sub">{shortcut.description}</div>{/if}
  </div>
  <KeyChips keys={shortcut.display} {os} />
</li>
