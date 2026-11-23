# VSCode configuration

## settings.json

```json
{
    "search.exclude": {
        "**/target/**": true,
        "**/sqlite3/**": true,
        "**/node_modules/**": true,
        "**/dist/**": true
    },
    "files.watcherExclude": {
        "**/target/**": true,
        "**/sqlite3/**": true,
        "**/node_modules/**": true,
        "**/dist/**": true
    },

    // Rust extensions settings
    "[rust]": {
        "editor.defaultFormatter": "rust-lang.rust-analyzer",
        "editor.formatOnSave": true
    },
    "[toml]": {
        "editor.defaultFormatter": "tamasfe.even-better-toml",
        "editor.formatOnSave": true
    },
    "rust-analyzer.rustfmt.extraArgs": ["+nightly-2026-11-14"],
    "rust-analyzer.check.overrideCommand": ["cargo", "xtask", "check-editor"],

    // Web extensions settings
    "[dockerfile]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[html]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[css]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[javascript]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[javascriptreact]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[typescript]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[typescriptreact]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[markdown]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[json]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[jsonc]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[yaml]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "[xml]": {
        "editor.defaultFormatter": "dprint.dprint",
        "editor.formatOnSave": true
    },
    "files.associations": {
        "*.entitlements": "xml",
        "*.manifest": "xml",
        "*.plist": "xml",
        "*.svg": "xml",
        "*.xaml": "xml"
    },

    // Clang-format extension
    "[c]": {
        "editor.defaultFormatter": "xaver.clang-format",
        "editor.formatOnSave": true
    },
    "[cpp]": {
        "editor.defaultFormatter": "xaver.clang-format",
        "editor.formatOnSave": true
    },
    "[objective-c]": {
        "editor.defaultFormatter": "xaver.clang-format",
        "editor.formatOnSave": true
    },
    "[objective-cpp]": {
        "editor.defaultFormatter": "xaver.clang-format",
        "editor.formatOnSave": true
    },
    "[java]": {
        "editor.defaultFormatter": "xaver.clang-format",
        "editor.formatOnSave": true
    }
}
```

## extensions.json

```json
{
    "recommendations": [
        "dprint.dprint",
        "EditorConfig.EditorConfig",
        "rust-lang.rust-analyzer",
        "tamasfe.even-better-toml",
        "xaver.clang-format"
    ]
}
```

## tasks.json

```json
{
    "version": "2.0.0",
    "tasks": [
        {
            "label": "Configure VS Code",
            "type": "process",
            "command": "cargo",
            "args": ["xtask", "configure-vscode"],
            "problemMatcher": [],
            "presentation": {
                "reveal": "never",
                "panel": "dedicated",
                "close": true
            },
            "runOptions": {
                "runOn": "folderOpen"
            }
        }
    ]
}
```

Run `cargo xtask configure-vscode` or restart VSCode after creating the files above.
It builds the dprint WebAssembly plugin, enables format-on-save for web and XML files and Dockerfiles,
and sets `rust-analyzer.files.exclude` for packages unsupported on the current platform.
