import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

// Le daemon sert la PWA depuis `pwa/dist` (embarquée via rust-embed).
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2020',
    cssMinify: true,
    assetsInlineLimit: 0,
  },
  server: {
    // Développement : proxy vers le daemon local.
    proxy: {
      '/api': 'http://127.0.0.1:18450',
      '/ws': { target: 'ws://127.0.0.1:18450', ws: true },
      '/ca.crt': 'http://127.0.0.1:18450',
    },
  },
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts'],
  },
});
