import { defineConfig } from 'vite';

// The LAN relay of the server (npm run server) next to the page, like in production
const proxy = { '/relay': { target: 'ws://127.0.0.1:8080', ws: true } };

export default defineConfig({
    base: './',
    build: {
        outDir: 'dist',
    },
    server: { proxy },
    preview: { proxy },
});
