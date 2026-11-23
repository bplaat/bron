/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { type RefObject } from 'preact';
import { useEffect, useRef } from 'preact/hooks';

// Returns a ref to attach to a container; while `active`, a pointer press
// outside that container calls `onClose`.
export function useClickOutside<T extends HTMLElement>(active: boolean, onClose: () => void): RefObject<T | null> {
    const ref = useRef<T>(null);
    const onCloseRef = useRef(onClose);
    onCloseRef.current = onClose;

    useEffect(() => {
        if (!active) return;
        function onDown(e: MouseEvent) {
            if (ref.current && !ref.current.contains(e.target as Node)) onCloseRef.current();
        }
        document.addEventListener('mousedown', onDown);
        return () => document.removeEventListener('mousedown', onDown);
    }, [active]);

    return ref;
}
