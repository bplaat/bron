/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

declare global {
    interface Window {
        ipc: EventTarget & {
            postMessage: (message: string) => void;
            addEventListener: (
                type: 'message',
                listener: (event: MessageEvent) => void,
                options?: boolean | AddEventListenerOptions,
            ) => void;
            removeEventListener: (
                type: 'message',
                listener: (event: MessageEvent) => void,
                options?: boolean | EventListenerOptions,
            ) => void;
        };
    }
}

export type IpcType = 'ipc' | 'websocket';

// The token of the LAN server from the QR-code URL, kept for page reloads
export const ipcToken =
    new URLSearchParams(window.location.search).get('token') ?? sessionStorage.getItem('ipcToken') ?? '';
sessionStorage.setItem('ipcToken', ipcToken);

export class Ipc {
    type: IpcType;
    ws?: WebSocket;
    pending = new Map<string, Promise<object>>();

    constructor() {
        if ('ipc' in window) {
            this.type = 'ipc';
        } else {
            this.type = 'websocket';
            this.ws = new WebSocket(`/ipc?token=${encodeURIComponent(ipcToken)}`);
        }
    }

    send(type: string, data: { [key: string]: any } = {}) {
        const message = JSON.stringify({ type, ...data });
        return new Promise((resolve) => {
            if (this.type === 'ipc') {
                window.ipc.postMessage(message);
                resolve(undefined);
            }
            if (this.type === 'websocket') {
                if (this.ws!.readyState === WebSocket.OPEN) {
                    this.ws!.send(message);
                    resolve(undefined);
                } else {
                    this.ws!.addEventListener(
                        'open',
                        () => {
                            this.ws!.send(message);
                            resolve(undefined);
                        },
                        { once: true },
                    );
                }
            }
        });
    }

    on(type: string, callback: (data: object) => void) {
        const listener = (event: MessageEvent) => {
            const { type: receivedType, ...data } = JSON.parse(event.data);
            if (receivedType === type) {
                console.debug(`Recv ${event.data}`);
                callback(data);
            }
        };
        if (this.type === 'ipc') window.ipc.addEventListener('message', listener);
        if (this.type === 'websocket') this.ws!.addEventListener('message', listener);
        return {
            remove: () => {
                if (this.type === 'ipc') window.ipc.removeEventListener('message', listener);
                if (this.type === 'websocket') this.ws!.removeEventListener('message', listener);
            },
        };
    }

    // Requests of the same type are sent one at a time, so each response matches its request
    request(type: string, data: { [key: string]: any } = {}) {
        const response = (this.pending.get(type) ?? Promise.resolve()).then(
            () =>
                new Promise<object>((resolve) => {
                    const listener = this.on(`${type}Response`, (data) => {
                        listener.remove();
                        resolve(data);
                    });
                    this.send(type, data);
                }),
        );
        this.pending.set(type, response);
        return response;
    }
}
