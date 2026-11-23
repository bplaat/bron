/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { type ComponentChildren, type ComponentType } from 'preact';
import { type JSX } from 'preact/jsx-runtime';
import { cx } from '../utils.ts';
import { type IconProps } from './icons.tsx';
import './sidebar.css';

export interface SidebarLayoutProps {
    children: ComponentChildren;
    navbar: ComponentChildren;
    sidebar: ComponentChildren;
    version?: string;
}

export function SidebarLayout({ children, navbar, sidebar, version }: SidebarLayoutProps) {
    return (
        <div class="layout">
            {navbar}
            <div class="layout-body">
                <aside class="sidebar">
                    <nav class="sidebar-nav">{sidebar}</nav>
                    <div class="spacer" />
                    {version && <p class="sidebar-version">v{version}</p>}
                </aside>
                <main class="layout-main">{children}</main>
            </div>
        </div>
    );
}

export type SidebarLinkProps = Omit<JSX.IntrinsicElements['a'], 'href' | 'icon' | 'role'> & {
    href: string;
    label: string;
    icon: ComponentType<Omit<IconProps, 'path'>>;
    active?: boolean;
};

export function SidebarLink({ label, icon, active = false, class: extraClass, ...props }: SidebarLinkProps) {
    const LinkIcon = icon;
    return (
        <a {...props} title={props.title ?? label} class={cx('sidebar-item', active && 'is-active', extraClass)}>
            <LinkIcon class="is-md" />
            <span class="sidebar-label">{label}</span>
        </a>
    );
}
