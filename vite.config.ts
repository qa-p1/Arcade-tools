import { defineConfig } from 'vite';
export default defineConfig({
  clearScreen: false,
  server: { port: 1421, strictPort: true, watch: { ignored: ['**/src-tauri/**', '**/target/**', '**/src/**'] } },
  build: { target: ['es2021', 'safari15'], sourcemap: false, modulePreload: { polyfill: false } },
});
