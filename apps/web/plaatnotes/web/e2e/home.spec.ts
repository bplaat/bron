/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { expect, test, type Page } from '@playwright/test';

const API_URL = `http://localhost:${process.env.PLAYWRIGHT_PORT ?? '8080'}/api`;

test.describe('Home', () => {
    test('loads home page with correct title', async ({ page }) => {
        await page.goto('/');
        await expect(page).toHaveTitle('PlaatNotes');
    });

    test('shows empty state or notes grid', async ({ page }) => {
        await page.goto('/');
        const emptyState = page.getByText('No notes yet. Create one!');
        const firstNote = page.locator('a[href^="/notes/"]').first();
        await expect(emptyState.or(firstNote)).toBeVisible();
    });

    test('FAB button navigates to create note page', async ({ page }) => {
        await page.goto('/');
        await page.getByTitle('Create note').click();
        await expect(page).toHaveURL('/notes/create');
        await expect(page).toHaveTitle(/PlaatNotes.*Create Note/);
    });

    test('search bar is visible and filters notes', async ({ page }) => {
        await page.goto('/');
        const searchInput = page.getByPlaceholder('Search notes…');
        await expect(searchInput).toBeVisible();

        await searchInput.fill('__nonexistent_query_xyz__');
        await expect(page.getByText('No notes match your search.')).toBeVisible();

        await searchInput.clear();
    });

    test('navbar shows user avatar and name', async ({ page }) => {
        await page.goto('/');
        // Avatar initials: TU for Test User
        await expect(page.locator('header').getByText('TU')).toBeVisible();
        await expect(page.locator('header').getByText('Test User')).toBeVisible();
    });

    test('user dropdown menu contains Settings and Sign out', async ({ page }) => {
        await page.goto('/');
        await page
            .locator('header button')
            .filter({ has: page.locator('.avatar') })
            .click();
        await expect(page.getByRole('button', { name: 'Settings' })).toBeVisible();
        await expect(page.getByRole('button', { name: 'Sign out' })).toBeVisible();
    });

    test('sidebar links navigate correctly', async ({ page }) => {
        await page.goto('/');
        await page.getByRole('link', { name: 'Archive', exact: true }).click();
        await expect(page).toHaveURL('/archive');

        await page.getByRole('link', { name: 'Trash', exact: true }).click();
        await expect(page).toHaveURL('/trash');

        await page.getByRole('link', { name: 'Notes', exact: true }).click();
        await expect(page).toHaveURL('/');
    });

    test('unknown route shows not-found page', async ({ page }) => {
        await page.goto('/this-route-does-not-exist');
        await expect(page.getByRole('heading', { name: '404' })).toBeVisible();
        await expect(page.getByText('No idea how you got here')).toBeVisible();
        await expect(page.getByRole('button', { name: 'Go home' })).toBeVisible();
    });
});

test.describe('Home - Note card actions', () => {
    async function authState(page: Page): Promise<{ token: string; userId: string; headers: Record<string, string> }> {
        if (!page.url().startsWith('http')) await page.goto('/');
        const token = await page.evaluate(() => localStorage.getItem('token') ?? '');
        const headers = { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' };
        const res = await page.request.get(`${API_URL}/auth/validate`, { headers });
        const { user } = await res.json();
        return { token, userId: user.id, headers };
    }

    async function createNote(page: Page, fields: Record<string, any>): Promise<{ id: string }> {
        const { userId, headers } = await authState(page);
        const res = await page.request.post(`${API_URL}/users/${userId}/notes`, {
            headers,
            data: JSON.stringify(fields),
        });
        return res.json();
    }

    async function noteOrder(page: Page, labels: string[]): Promise<string[]> {
        return page.locator('a[href^="/notes/"]').evaluateAll((els, expectedLabels) => {
            const order: string[] = [];
            for (const el of els) {
                const text = el.textContent ?? '';
                for (const label of expectedLabels) {
                    if (text.includes(label)) {
                        order.push(label);
                        break;
                    }
                }
            }
            return order;
        }, labels);
    }

    async function deleteNote(page: Page, id: string): Promise<void> {
        const { headers } = await authState(page);
        await page.request.delete(`${API_URL}/notes/${id}`, { headers });
    }

    test('pin a note via note card button on home page', async ({ page }) => {
        const note = await createNote(page, { body: 'Card pin test note', title: 'Card Pin Note' });

        await page.goto('/');
        const card = page.locator('a[href^="/notes/"]').filter({ hasText: 'Card Pin Note' });
        await card.hover();
        await card.getByTitle('Pin').click();

        // Pinned section should now appear
        await expect(page.getByRole('heading', { name: 'Pinned', exact: true })).toBeVisible();
        await expect(card.getByTitle('Unpin', { exact: true })).toBeVisible();

        await deleteNote(page, note.id);
    });

    test('archive a note via note card button on home page', async ({ page }) => {
        const note = await createNote(page, { body: 'Card archive test note', title: 'Card Archive Note' });

        await page.goto('/');
        const card = page.locator('a[href^="/notes/"]').filter({ hasText: 'Card Archive Note' });
        await card.hover();
        await card.getByTitle('Archive').click();

        await expect(page.locator('a[href^="/notes/"]').filter({ hasText: 'Card Archive Note' })).not.toBeVisible();

        await deleteNote(page, note.id);
    });

    test('trash a note via note card button on home page', async ({ page }) => {
        const note = await createNote(page, { body: 'Card trash test note', title: 'Card Trash Note' });

        await page.goto('/');
        const card = page.locator('a[href^="/notes/"]').filter({ hasText: 'Card Trash Note' });
        await card.hover();
        await card.getByTitle('Move to trash').click();

        await expect(page.locator('a[href^="/notes/"]').filter({ hasText: 'Card Trash Note' })).not.toBeVisible();

        await deleteNote(page, note.id);
    });

    test('"Pinned" section heading visible when pinned notes exist', async ({ page }) => {
        const { headers } = await authState(page);
        const note = await createNote(page, { body: 'Pinned section test' });
        await page.request.put(`${API_URL}/notes/${note.id}`, {
            headers,
            data: JSON.stringify({ body: 'Pinned section test', isPinned: true, isArchived: false, isTrashed: false }),
        });

        await page.goto('/');
        await expect(page.getByRole('heading', { name: 'Pinned' })).toBeVisible();

        await deleteNote(page, note.id);
    });

    test('dragging notes reorders them and persists after reload', async ({ page }) => {
        const labelA = `Home reorder A ${Date.now()}`;
        const labelB = `Home reorder B ${Date.now()}`;
        const noteA = await createNote(page, { title: labelA, body: `Body ${labelA}` });
        const noteB = await createNote(page, { title: labelB, body: `Body ${labelB}` });

        await page.goto('/');
        await expect.poll(() => noteOrder(page, [labelA, labelB])).toEqual([labelB, labelA]);

        const reorderPromise = page.waitForResponse(
            (res) =>
                res.url().includes('/notes/reorder') &&
                !res.url().includes('/notes/archived/reorder') &&
                !res.url().includes('/notes/pinned/reorder') &&
                res.request().method() === 'PUT',
        );
        await page
            .locator('div[draggable="true"]')
            .filter({ hasText: labelA })
            .dragTo(page.locator('div[draggable="true"]').filter({ hasText: labelB }));
        await reorderPromise;

        await expect.poll(() => noteOrder(page, [labelA, labelB])).toEqual([labelA, labelB]);

        await page.reload();
        await expect.poll(() => noteOrder(page, [labelA, labelB])).toEqual([labelA, labelB]);

        await deleteNote(page, noteA.id);
        await deleteNote(page, noteB.id);
    });

    test('normal user is redirected away from admin users page', async ({ page }) => {
        await page.goto('/admin/users');
        await expect(page).toHaveURL('/');
        await expect(page.getByRole('heading', { name: 'Users' })).not.toBeVisible();
    });
});
