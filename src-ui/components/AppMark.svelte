<script lang="ts">
  import { appIcon, glyph } from '../lib/glyphs';

  // An app's own coloured icon, as the desktop shows it; the monochrome glyph tile for anything else.
  let { id, size = 'md' }: { id: string; size?: 'sm' | 'md' | 'lg' } = $props();
  const icon = $derived(appIcon(id));
  const glyphSize = $derived(size === 'lg' ? 26 : size === 'sm' ? 15 : 18);
</script>

{#if icon}
  <img class="app-mark {size}" src={icon} alt="" />
{:else}
  <span class="glyph-tile" class:sm={size === 'sm'} class:lg={size === 'lg'}><img src={glyph(id)} alt="" width={glyphSize} height={glyphSize} /></span>
{/if}

<style>
  .app-mark { display: block; flex: none; width: var(--tile); height: var(--tile); filter: drop-shadow(0 1px 1.5px rgb(0 0 0 / .14)); }
  .app-mark.sm { width: var(--tile-sm); height: var(--tile-sm); }
  .app-mark.lg { width: var(--tile-lg); height: var(--tile-lg); }
</style>
