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
    "rust-analyzer.rustfmt.extraArgs": ["+nightly"],
    "rust-analyzer.check.overrideCommand": ["cargo", "xtask", "check-editor"],

    // Web extensions settings
    "[html]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[css]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[javascript]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[javascriptreact]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[typescript]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[typescriptreact]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[markdown]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[json]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[yaml]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
    },
    "[xml]": {
        "editor.defaultFormatter": "esbenp.prettier-vscode",
        "editor.formatOnSave": true
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
        "EditorConfig.EditorConfig",
        "rust-lang.rust-analyzer",
        "tamasfe.even-better-toml",
        "esbenp.prettier-vscode",
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
It adds `rust-analyzer.files.exclude` for packages unsupported on the current platform.
