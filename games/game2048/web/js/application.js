/*
 * Copyright (c) 2014 Gabriele Cirulli
 *
 * SPDX-License-Identifier: MIT
 */

// Start the game with the stored state, once the browser is ready to render it (avoids glitches)
window.ipc.addEventListener('message', function listener(event) {
    const message = JSON.parse(event.data);
    if (message.type !== 'state') return;
    window.ipc.removeEventListener('message', listener);
    window.requestAnimationFrame(function () {
        new GameManager(4, KeyboardInputManager, HTMLActuator, new StorageManager(message));
    });
});
window.ipc.postMessage(JSON.stringify({ type: 'ready' }));

window.addEventListener('contextmenu', (event) => event.preventDefault());
