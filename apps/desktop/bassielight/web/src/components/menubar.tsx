/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { signal } from '@preact/signals';
import { useContext, useEffect, useState } from 'preact/hooks';
import { Link, useRoute } from 'wouter-preact';
import { IpcContext } from '../app.tsx';
import { ipcToken } from '../ipc.ts';
import { CogIcon, MotionPlayOutlineIcon, QrcodeIcon, SquareEditOutlineIcon } from './icons.tsx';
import { QrModal } from './qrmodal.tsx';
import './menubar.css';

export const $dmxLive = signal(false);

type UsbStatus =
    | { state: 'connected' }
    | { state: 'disconnected' }
    | {
          state: 'error';
          category: 'access' | 'busy' | 'noDevice' | 'timeout' | 'pipe' | 'unsupported' | 'other';
      };

const USB_ERRORS: Record<Extract<UsbStatus, { state: 'error' }>['category'], { label: string; detail: string }> = {
    access: { label: 'uDMX access denied', detail: 'Check USB permissions and reconnect.' },
    busy: { label: 'uDMX is busy', detail: 'Close other apps using uDMX.' },
    noDevice: { label: 'uDMX disconnected', detail: 'Reconnect uDMX; output resumes automatically.' },
    timeout: { label: 'uDMX timeout', detail: 'The adapter did not respond; retrying.' },
    pipe: { label: 'uDMX transfer stalled', detail: 'The USB transfer stalled; retrying.' },
    unsupported: { label: 'uDMX unsupported', detail: 'Check the installed USB driver.' },
    other: { label: 'uDMX error', detail: 'USB communication failed; retrying.' },
};

function usbStatusContent(status: UsbStatus) {
    if (status.state === 'connected') {
        return { label: 'uDMX connected', detail: 'USB output is ready.' };
    }
    if (status.state === 'disconnected') {
        return { label: 'uDMX not connected', detail: 'Connect uDMX; retrying automatically.' };
    }
    return USB_ERRORS[status.category];
}

function NavLink({ href, children }: { href: string; children: any }) {
    const [isActive] = useRoute(href);
    return (
        <Link href={href} class={`menubar-item ${isActive ? 'is-active' : ''}`}>
            {children}
        </Link>
    );
}

export function Menubar() {
    const ipc = useContext(IpcContext)!;
    const [showQrCode, setShowQrCode] = useState(false);
    const [usbStatus, setUsbStatus] = useState<UsbStatus>({ state: 'disconnected' });

    useEffect(() => {
        const listeners = [
            ipc.on('start', () => ($dmxLive.value = true)),
            ipc.on('stop', () => ($dmxLive.value = false)),
            ipc.on('usbStatusChanged', (data) => {
                setUsbStatus((data as { status: UsbStatus }).status);
            }),
        ];
        ipc.request('getUsbStatus').then(({ status }: any) => setUsbStatus(status));
        return () => listeners.forEach((l) => l.remove());
    }, []);

    const statusContent = usbStatusContent(usbStatus);
    const usbStatusClass =
        usbStatus.state === 'connected' ? 'is-success' : usbStatus.state === 'error' ? 'is-warning' : 'is-danger';

    return (
        <>
            <div id="menubar" class="menubar">
                <h1 class="menubar-title">BassieLight</h1>

                <NavLink href="/">
                    <MotionPlayOutlineIcon />
                    Stage
                </NavLink>
                <NavLink href="/editor">
                    <SquareEditOutlineIcon />
                    Editor
                </NavLink>
                <NavLink href="/settings">
                    <CogIcon />
                    Settings
                </NavLink>

                <div class="spacer" />

                <div class="menubar-status" role="status" title={statusContent.detail}>
                    <span class={`menubar-dot ${usbStatusClass}`} />
                    {statusContent.label}
                </div>

                <div class="menubar-status">
                    <span class={`menubar-dot ${$dmxLive.value ? 'is-success' : 'is-danger'}`} />
                    {$dmxLive.value ? 'DMX is live' : 'DMX is off'}
                </div>

                <button class="menubar-item" onClick={() => setShowQrCode(true)}>
                    <QrcodeIcon />
                    QR-code
                </button>
            </div>
            {showQrCode && (
                <QrModal
                    contents={`http://${window.location.host}/?token=${ipcToken}`}
                    onClose={() => setShowQrCode(false)}
                />
            )}
        </>
    );
}
