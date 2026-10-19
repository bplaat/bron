/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

import DOMPurify from 'dompurify';
import { marked } from 'marked';
import {
    CodeBracesIcon,
    CodeTagsIcon,
    FormatBoldIcon,
    FormatItalicIcon,
    FormatListBulletedIcon,
    FormatListNumberedIcon,
    FormatQuoteOpenIcon,
    FormatStrikethroughIcon,
    FormatUnderlineIcon,
    LinkIcon,
    MinusIcon,
} from 'plaatui';
import { useEffect, useRef, useState } from 'preact/hooks';
import TurndownService from 'turndown';
import { t } from '../services/i18n.service.ts';
import './rich-editor.css';

const td = new TurndownService({ headingStyle: 'atx', bulletListMarker: '-' });
td.addRule('underline', {
    filter: ['u'],
    replacement: (content) => `<u>${content}</u>`,
});
td.addRule('strikethrough', {
    // @ts-ignore
    filter: ['s', 'strike', 'del'],
    replacement: (content) => `~~${content}~~`,
});
// Preserve <br> as HTML so empty lines round-trip correctly
td.addRule('br', {
    filter: 'br',
    replacement: () => '<br>',
});
// Empty paragraphs (blank lines in editor) become <br> placeholders in markdown
td.addRule('empty-paragraph', {
    filter: (node) => node.nodeName === 'P' && !node.textContent?.trim(),
    replacement: () => '<br>',
});
// Use --- for horizontal rules
td.addRule('hr', {
    filter: 'hr',
    replacement: () => '\n\n---\n\n',
});

interface ToolbarButtonProps {
    onClick: () => void;
    title: string;
    children: preact.ComponentChildren;
}

function ToolbarButton({ onClick, title, children }: ToolbarButtonProps) {
    return (
        <button
            type="button"
            onMouseDown={(e) => {
                e.preventDefault();
                onClick();
            }}
            title={title}
            class="editor-tool"
        >
            {children}
        </button>
    );
}

function ToolbarSep() {
    return <div class="editor-sep" />;
}

interface RichEditorProps {
    value: string;
    onInput: (markdown: string) => void;
    placeholder?: string;
    class?: string;
    autoFocus?: boolean;
}

export function RichEditor({ value, onInput, placeholder, class: className, autoFocus }: RichEditorProps) {
    const editorRef = useRef<HTMLDivElement>(null);
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const lastEmittedRef = useRef<string>(value);
    const [isEmpty, setIsEmpty] = useState(!value.trim());
    const [plainMode, setPlainMode] = useState(() => localStorage.getItem('editor-mode') === 'plain');
    const switchedToRichRef = useRef(false);

    useEffect(() => {
        if (!editorRef.current) return;
        editorRef.current.innerHTML = DOMPurify.sanitize(marked.parse(value || '') as string);
        setIsEmpty(!value.trim());
    }, []);

    // Programmatic autofocus on mount - works on every SPA navigation unlike HTML autofocus attribute
    useEffect(() => {
        if (!autoFocus) return;
        const id = requestAnimationFrame(() => {
            const active = document.activeElement;
            if (active && active !== document.body) return;
            if (plainMode) textareaRef.current?.focus();
            else editorRef.current?.focus();
        });
        return () => cancelAnimationFrame(id);
    }, []);

    useEffect(() => {
        if (!editorRef.current) return;
        setIsEmpty(!value.trim());
        if (value !== lastEmittedRef.current) {
            lastEmittedRef.current = value;
            // Don't reset DOM while the editor has focus - user is actively typing
            // and their local content is authoritative. Only update for external changes.
            if (!editorRef.current.contains(document.activeElement)) {
                editorRef.current.innerHTML = DOMPurify.sanitize(marked.parse(value || '') as string);
            }
        }
    }, [value]);

    // When switching back to rich mode, re-render the latest markdown into the editor
    useEffect(() => {
        if (switchedToRichRef.current && editorRef.current) {
            switchedToRichRef.current = false;
            editorRef.current.innerHTML = DOMPurify.sanitize(marked.parse(value || '') as string);
            lastEmittedRef.current = value;
        }
    }, [plainMode]);

    function emit() {
        if (!editorRef.current) return;
        const markdown = td.turndown(editorRef.current.innerHTML);
        lastEmittedRef.current = markdown;
        onInput(markdown);
    }

    function execCmd(cmd: string, val?: string) {
        document.execCommand(cmd, false, val);
        emit();
    }

    function handleLink() {
        const url = prompt(t('editor.link_prompt'));
        if (url) execCmd('createLink', url);
    }

    function insertCode(block: boolean) {
        // Serialize through the DOM so the selected text is escaped, not parsed as HTML
        const code = document.createElement('code');
        code.textContent = window.getSelection()?.toString() || 'code';
        execCmd('insertHTML', block ? `<pre>${code.outerHTML}</pre>` : code.outerHTML);
    }

    function togglePlainMode() {
        if (plainMode) {
            switchedToRichRef.current = true;
        }
        const next = !plainMode;
        localStorage.setItem('editor-mode', next ? 'plain' : 'rich');
        setPlainMode(next);
    }

    return (
        <div class={`editor ${className || ''}`}>
            <div class="editor-toolbar">
                {!plainMode && (
                    <>
                        <ToolbarButton onClick={() => execCmd('bold')} title={t('editor.bold')}>
                            <FormatBoldIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('italic')} title={t('editor.italic')}>
                            <FormatItalicIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('underline')} title={t('editor.underline')}>
                            <FormatUnderlineIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('strikeThrough')} title={t('editor.strikethrough')}>
                            <FormatStrikethroughIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarSep />
                        <ToolbarButton onClick={() => execCmd('formatBlock', 'h1')} title={t('editor.h1')}>
                            <span class="editor-tool-text">H1</span>
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('formatBlock', 'h2')} title={t('editor.h2')}>
                            <span class="editor-tool-text">H2</span>
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('formatBlock', 'h3')} title={t('editor.h3')}>
                            <span class="editor-tool-text">H3</span>
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('formatBlock', 'p')} title={t('editor.p')}>
                            <span class="editor-tool-text is-normal">¶</span>
                        </ToolbarButton>
                        <ToolbarButton
                            onClick={() => execCmd('formatBlock', 'blockquote')}
                            title={t('editor.blockquote')}
                        >
                            <FormatQuoteOpenIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarSep />
                        <ToolbarButton onClick={() => execCmd('insertUnorderedList')} title={t('editor.ul')}>
                            <FormatListBulletedIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarButton onClick={() => execCmd('insertOrderedList')} title={t('editor.ol')}>
                            <FormatListNumberedIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarSep />
                        <ToolbarButton onClick={() => insertCode(false)} title={t('editor.code')}>
                            <CodeTagsIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarButton onClick={() => insertCode(true)} title={t('editor.code_block')}>
                            <CodeBracesIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarSep />
                        <ToolbarButton onClick={handleLink} title={t('editor.link')}>
                            <LinkIcon class="is-sm" />
                        </ToolbarButton>
                        <ToolbarSep />
                        <ToolbarButton onClick={() => execCmd('insertHorizontalRule')} title={t('editor.hr')}>
                            <MinusIcon class="is-sm" />
                        </ToolbarButton>
                    </>
                )}
                <div class="spacer" />
                <button
                    type="button"
                    onMouseDown={(e) => {
                        e.preventDefault();
                        togglePlainMode();
                    }}
                    title={plainMode ? t('editor.rich_mode') : t('editor.plain_mode')}
                    class="editor-mode-toggle"
                >
                    <CodeTagsIcon class="is-xs" />
                    {plainMode ? t('editor.rich_mode') : t('editor.plain_mode')}
                </button>
            </div>
            {plainMode ? (
                <textarea
                    ref={textareaRef}
                    class="editor-textarea"
                    placeholder={placeholder}
                    value={value}
                    onKeyDown={(e) => {
                        if (e.key === 'Tab') {
                            e.preventDefault();
                            const el = e.target as HTMLTextAreaElement;
                            const start = el.selectionStart;
                            const end = el.selectionEnd;
                            const markdown = el.value.substring(0, start) + '    ' + el.value.substring(end);
                            el.value = markdown;
                            el.selectionStart = el.selectionEnd = start + 4;
                            lastEmittedRef.current = markdown;
                            onInput(markdown);
                        }
                    }}
                    onInput={(e) => {
                        const markdown = (e.target as HTMLTextAreaElement).value;
                        lastEmittedRef.current = markdown;
                        onInput(markdown);
                    }}
                />
            ) : (
                <div class="editor-body">
                    {isEmpty && <p class="editor-placeholder">{placeholder}</p>}
                    <div
                        ref={editorRef}
                        contentEditable={true}
                        class="rich-editor-content"
                        onKeyDown={(e) => {
                            if (e.key === 'Tab') {
                                e.preventDefault();
                                document.execCommand('insertText', false, '    ');
                            }
                        }}
                        onInput={emit}
                    />
                </div>
            )}
        </div>
    );
}
