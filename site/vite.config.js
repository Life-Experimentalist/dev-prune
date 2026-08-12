import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The site is served from the root of a custom domain (see public/CNAME), so absolute
// asset paths are correct — and they have to be, because the prerendered HTML is also
// what crawlers resolve `/assets/...` against.
export default defineConfig({
  plugins: [react()],
  base: '/',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
