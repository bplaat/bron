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
    "files.associations": {
        "*.entitlements": "xml",
        "*.manifest": "xml",
        "*.plist": "xml",
        "*.svg": "xml",
        "*.xaml": "xml"
    },

    // dprint formats everything except Rust, Swift and the clang-format languages
    "editor.defaultFormatter": "dprint.dprint",
    "editor.formatOnSave": true,
    "[rust]": {
        "editor.defaultFormatter": "rust-lang.rust-analyzer"
    },
    "[swift]": {
        "editor.defaultFormatter": "swiftlang.swift-vscode"
    },
    "[c][cpp][java][objective-c][objective-cpp]": {
        "editor.defaultFormatter": "xaver.clang-format"
    },
    "rust-analyzer.rustfmt.extraArgs": ["+nightly-2026-11-14"],
    "rust-analyzer.check.overrideCommand": ["cargo", "xtask", "check-editor"]
}
```

## extensions.json

```json
{
    "recommendations": [
        "13xforever.language-x86-64-assembly",
        "dprint.dprint",
        "EditorConfig.EditorConfig",
        "mathiasfrohlich.Kotlin",
        "Okerew.shader-with-metal",
        "PolyMeilex.wgsl",
        "rust-lang.rust-analyzer",
        "swiftlang.swift-vscode",
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
It builds the dprint WebAssembly plugins, makes dprint the default formatter on save,
and sets `rust-analyzer.files.exclude` for packages unsupported on the current platform.
