/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { useEffect } from 'preact/hooks';

export function SettingsPage() {
    useEffect(() => {
        document.title = 'BassieLight - Settings';
    }, []);

    return (
        <div class="section">
            <h2 class="title">Settings</h2>
            <p class="block">
                Made by{' '}
                <a href="https://bplaat.nl/" target="_blank">
                    Bastiaan van der Plaat
                </a>{' '}
                and{' '}
                <a href="https://lplaat.nl/" target="_blank">
                    Leonard van der Plaat
                </a>
            </p>
        </div>
    );
}
