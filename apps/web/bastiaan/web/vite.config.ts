/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { defineConfig } from 'vite';

// The LAN relay of the server (cargo run --release -p bastiaan) next to the page, like in production
const proxy = { '/relay': { target: 'ws://127.0.0.1:8080', ws: true } };

// The plain website in index.html runs no script, its age is filled in at build time (born on 1 July 2002)
function age() {
    const now = new Date();
    return now.getFullYear() - 2002 - (now.getMonth() < 6 ? 1 : 0);
}

export default defineConfig({
    base: './',
    build: {
        outDir: 'dist',
    },
    plugins: [
        { name: 'age', transformIndexHtml: { order: 'pre', handler: (html) => html.replace('%AGE%', String(age())) } },
    ],
    server: { proxy },
    preview: { proxy },
});
