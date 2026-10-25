<script lang="ts" module>
  // 24×24 outline icons drawn with the current text colour.
  const paths = {
    home: 'M3 10.5 12 3l9 7.5M5 9v11h5v-6h4v6h5V9',
    apps: 'M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z',
    updates: 'M12 3v12m0 0-4.5-4.5M12 15l4.5-4.5M4 17v3h16v-3',
    keyboard: 'M3 6h18v12H3zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
    book: 'M4 5a2 2 0 0 1 2-2h13v15H6a2 2 0 0 0-2 2zm0 15a2 2 0 0 0 2 1h13',
    link: 'M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1',
    engine: 'M8 4h8v4h3v8h-3v4H8v-4H5V8h3zM10 10h4v4h-4z',
    settings: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
    sparkle: 'M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8zM19 16l.8 2.2L22 19l-2.2.8L19 22l-.8-2.2L16 19l2.2-.8z',
    search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zm9 2-4-4',
    refresh: 'M20 11a8 8 0 0 0-14.7-4.3L4 8m0-4v4h4M4 13a8 8 0 0 0 14.7 4.3L20 16m0 4v-4h-4',
    play: 'M7 4.5v15l12-7.5z',
    moon: 'M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z',
    wrench: 'M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8-3-3 8-8-1.3-1.3a4 4 0 0 1-5-5L13 2.5zM3 21l6-6',
    trash: 'M4 7h16M10 11v6M14 11v6M5 7l1 13h12l1-13M9 7V4h6v3',
    external: 'M14 4h6v6M20 4l-9 9M18 14v6H4V6h6',
    chevronRight: 'M9 6l6 6-6 6',
    chevronLeft: 'M15 6l-6 6 6 6',
    chevronDown: 'M6 9l6 6 6-6',
    check: 'M5 12.5l4.5 4.5L19 7.5',
    alert: 'M12 9v4m0 4h.01M10.3 3.9 2.4 18a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z',
    x: 'M6 6l12 12M18 6 6 18',
    info: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zm0-5v-4m0-4h.01',
    edit: 'M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4',
    copy: 'M9 9h11v11H9zM5 15H4V4h11v1',
    power: 'M12 3v9M6.4 6.4a8 8 0 1 0 11.2 0',
    restart: 'M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4',
  } as const;
  export type IconName = keyof typeof paths;
</script>

<script lang="ts">
  let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<svg class="icon" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
