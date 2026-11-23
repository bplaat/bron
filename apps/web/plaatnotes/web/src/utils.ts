/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Dutch family-name particles (tussenvoegsels) sorted longest-first
// so that multi-word particles are matched before their shorter components.
// oxfmt-ignore
const SURNAME_PARTICLES = [
    'van der', 'van den', 'van de', 'van het', "van 't", 'van ter',
    'in het', "in 't", 'in de', 'in den',
    'op het', "op 't", 'op de', 'op den',
    'aan het', "aan 't", 'aan de', 'aan den',
    'uit het', "uit 't", 'uit de', 'uit den',
    'voor de', 'voor den', 'over de', 'over den', 'bij de', 'bij den',
    'van', 'de', 'den', 'der', 'het', "'t", 'te', 'ter', 'ten',
    'in', 'op', 'aan', 'uit', 'voor', 'over', 'bij',
];

/**
 * Returns the uppercase initial of the significant part of a last name,
 * stripping leading family-name particles/affixes.
 */
export function lastNameInitial(lastName: string): string {
    let name = lastName.trim();
    let progress = true;
    while (progress) {
        progress = false;
        const lower = name.toLowerCase();
        for (const particle of SURNAME_PARTICLES) {
            // Particle must be followed by a space and leave at least one character
            const prefix = particle + ' ';
            if (lower.startsWith(prefix) && name.length > prefix.length) {
                name = name.slice(prefix.length);
                progress = true;
                break;
            }
        }
    }
    return (name[0] ?? lastName[0] ?? '').toUpperCase();
}
