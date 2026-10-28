<script lang="ts" module>
  // The kit's 24×24 outline icons, drawn with the current text colour.
  import { icons, filledIcons, ICON_STROKE, type IconName } from '../design/icons';
  export type { IconName };
</script>

<script lang="ts">
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
  const filled = $derived(filledIcons.has(name));
</script>

<svg class="icon" width={size} height={size} viewBox="0 0 24 24" fill={filled ? 'currentColor' : 'none'} stroke={filled ? 'none' : 'currentColor'} stroke-width={ICON_STROKE} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={icons[name]} /></svg>
