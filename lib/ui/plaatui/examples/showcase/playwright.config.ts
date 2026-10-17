/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { defineConfig, devices } from '@playwright/test';

const port = process.env.PLAYWRIGHT_PORT ?? '4173';
const origin = `http://127.0.0.1:${port}`;

export default defineConfig({
    testDir: './e2e',
    fullyParallel: true,
    forbidOnly: !!process.env.CI,
    retries: process.env.CI ? 1 : 0,
    workers: process.env.CI ? 1 : undefined,
    reporter: process.env.CI ? [['list'], ['junit', { outputFile: 'test-results/junit.xml' }]] : 'list',
    projects: [
        { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
        {
            name: 'firefox',
            use: { ...devices['Desktop Firefox'] },
            // TODO: Re-enable Firefox on macOS once its startup hang is fixed upstream.
            // https://github.com/microsoft/playwright/issues/42082
            testIgnore: process.platform === 'darwin' ? '**/*' : undefined,
        },
        { name: 'webkit', use: { ...devices['Desktop Safari'] } },
    ],
    expect: {
        timeout: 10_000,
    },
    use: {
        baseURL: `${origin}/plaatui/`,
        trace: 'on-first-retry',
    },
    webServer: {
        command: `npm run dev -- --host 127.0.0.1 --port ${port}`,
        url: `${origin}/plaatui/`,
        reuseExistingServer: !process.env.CI,
    },
});
