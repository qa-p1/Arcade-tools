import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { readFileSync } from 'node:fs';

// The version of this build, shown in the footer and About.
const { version } = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
export default defineConfig({
  plugins: [svelte()],
  define: { __TOOLS_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: { port: 1421, strictPort: true, watch: { ignored: ['**/src-tauri/**', '**/target/**', '**/src/**'] } },
  build: {
    manifest: true,
    // The coloured app icons load as their own small files, so they don't count against the shell budget.
    assetsInlineLimit: (file) => (file.includes('/design/app-icons/') ? false : undefined),
    target: ['es2021', 'safari15'], sourcemap: false, modulePreload: { polyfill: false },
  },
});
