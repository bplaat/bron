/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { type ComponentChildren } from 'preact';
import { type JSX } from 'preact/jsx-runtime';
import { cx } from '../utils.ts';
import './navbar.css';

export function Navbar({ class: extraClass, children, ...props }: JSX.IntrinsicElements['header']) {
    return (
        <header {...props} class={cx('navbar', extraClass)}>
            <div class="navbar-container">{children}</div>
        </header>
    );
}

export type NavbarBrandProps = JSX.IntrinsicElements['a'] & {
    image: string;
    name: string;
};

export function NavbarBrand({ image, name, class: extraClass, ...props }: NavbarBrandProps) {
    return (
        <a {...props} class={cx('navbar-brand', extraClass)}>
            <img src={image} alt="" />
            <span class="navbar-brand-name">{name}</span>
        </a>
    );
}

export function NavbarSearch({ children }: { children: ComponentChildren }) {
    return (
        <div class="navbar-search">
            <div class="navbar-search-inner">{children}</div>
        </div>
    );
}

export function NavbarSpacer() {
    return <div class="spacer" />;
}

export function NavbarMenu({ class: extraClass, ...props }: JSX.IntrinsicElements['div']) {
    return <div {...props} class={cx('navbar-menu-wrapper', extraClass)} />;
}

export function NavbarUserButton({ class: extraClass, ...props }: JSX.IntrinsicElements['button']) {
    return <button type="button" {...props} class={cx('navbar-user', extraClass)} />;
}

export function NavbarUserName({ children }: { children: ComponentChildren }) {
    return <span class="navbar-user-name">{children}</span>;
}

export function Avatar({ class: extraClass, ...props }: JSX.IntrinsicElements['div']) {
    return <div {...props} class={cx('avatar', extraClass)} />;
}

export function DropdownMenu({ class: extraClass, ...props }: JSX.IntrinsicElements['div']) {
    return <div {...props} class={cx('dropdown-menu', extraClass)} />;
}

export function DropdownItem({ class: extraClass, ...props }: JSX.IntrinsicElements['button']) {
    return <button type="button" {...props} class={cx('dropdown-item', extraClass)} />;
}

export function DropdownDivider() {
    return <div class="dropdown-divider" />;
}
