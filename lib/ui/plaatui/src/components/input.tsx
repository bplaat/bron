/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import { type ComponentChildren } from 'preact';
import { type JSX } from 'preact/jsx-runtime';
import { cx } from '../utils.ts';
import { CheckIcon, ChevronDownIcon, CloseIcon, MagnifyIcon } from './icons.tsx';
import './input.css';

export function FormInput({ class: extraClass, ...props }: JSX.IntrinsicElements['input']) {
    return <input {...props} class={cx('input', extraClass)} />;
}

export function FormSelect({ class: extraClass, children, ...props }: JSX.IntrinsicElements['select']) {
    return (
        <span class="select-control">
            <select {...props} class={cx('select', extraClass)}>
                {children}
            </select>
            <ChevronDownIcon class="select-icon" />
        </span>
    );
}

export type FormCheckboxProps = Omit<JSX.IntrinsicElements['input'], 'type'> & {
    label: ComponentChildren;
};

export function FormCheckbox({ class: extraClass, label, ...props }: FormCheckboxProps) {
    return (
        <label class={cx('checkbox', extraClass)}>
            <input {...props} type="checkbox" class="checkbox-input" />
            <span class="checkbox-control">
                <CheckIcon class="checkbox-icon" />
            </span>
            <span class="checkbox-label">{label}</span>
        </label>
    );
}

export interface SearchInputProps {
    value: string;
    onInput: (value: string) => void;
    onClear: () => void;
    placeholder?: string;
}

export function SearchInput({ value, onInput, onClear, placeholder }: SearchInputProps) {
    return (
        <div class="search">
            <MagnifyIcon class="search-icon" />
            <input
                type="search"
                value={value}
                onInput={(e) => onInput((e.target as HTMLInputElement).value)}
                placeholder={placeholder}
                class={cx('input', value && 'has-value')}
            />
            {value && (
                <button type="button" onClick={onClear} class="search-clear">
                    <CloseIcon class="is-sm" />
                </button>
            )}
        </div>
    );
}
