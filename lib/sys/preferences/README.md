# Preferences

Minimal shared preferences for desktop and mobile, using native value types.

| Platform    | Backend                                       | Dependencies                      |
| ----------- | --------------------------------------------- | --------------------------------- |
| Android     | Native SharedPreferences through JNI          | Local `jni`                       |
| iOS / macOS | Native NSUserDefaults suite                   | Local `objc2`                     |
| Windows     | Registry values under `HKEY_CURRENT_USER`     | -                                 |
| Linux       | JSON file in the application config directory | Local `directories`, `serde_json` |

```rust,no_run
use preferences::{Preferences, Value};
let prefs = Preferences::new("nl.bplaat.example")?;
prefs.set("dark_mode", true)?;
prefs.set("launches", 42_i64)?;
prefs.set("volume", 0.75_f32)?;
prefs.set("username", "Bastiaan")?;
assert_eq!(prefs.get("dark_mode")?, Some(Value::Bool(true)));
prefs.remove("username")?;
# Ok::<(), std::io::Error>(())
```

Values are `bool`, `i64`, finite `f32`, or `String`. Floats use Android's native 32-bit precision. `get` returns
`None` for missing keys and an error for unsupported native values. Keys and strings support Unicode and embedded NUL
characters. The store is intended for small preferences, not secrets.

Application IDs are nonempty ASCII letters, digits, dots, underscores and hyphens, excluding `.` and `..`. Use a
reverse-DNS ID such as `nl.bplaat.example`.

## Storage

### Android

SharedPreferences named after the application ID. Writes use synchronous `commit` and report persistence failures.

### iOS / macOS

The NSUserDefaults suite of the application ID, or the standard defaults when it is the app's own bundle ID. Only
that domain is read, command-line arguments and registered or global defaults are ignored. Writes follow
NSUserDefaults' asynchronous persistence. Strings with embedded NUL characters are stored as UTF-8 NSData and keys
with them get an escaped `__preferences__` prefix. Keys starting with that prefix are escaped too, so distinct keys
cannot collide.

### Windows

Values in `HKEY_CURRENT_USER\Software\<organization>\<application>`, so `nl.bplaat.example` uses
`Software\bplaat\example` and IDs with fewer than three parts use `Software\<id>`. Booleans are `REG_DWORD` 0 or 1,
integers `REG_QWORD`, floats 4-byte `REG_BINARY` and strings `REG_SZ`. Each write sets one value. The key is shared by
32-bit and 64-bit builds and roams with the user profile. Vista and newer use `RegGetValueW`, `RegSetKeyValueW`
and `RegDeleteKeyValueW`; Windows XP builds load them at runtime and fall back to the open key API without them.

### Linux

`preferences.json` in `directories::ProjectDirs::from_path(id).config_dir()`. Writes read the current file,
synchronize a temporary file and atomically replace the destination; new directory entries and the replacement are
synchronized as well.

## Limitations

### Android

- The Java VM must be registered through `jni::vm::JavaVM::from_raw` first, the local `bwindow` does this already.
- The NDK exposes no Context, so the store gets the running Application from the non-SDK
  `ActivityThread.currentApplication()`, which is on the unsupported list and allowed for every target SDK.

### iOS / macOS

- Writes are persisted asynchronously by NSUserDefaults.

### Windows

- Keys are case-insensitive, `Theme` and `theme` are the same preference.
- Keys cannot contain NUL characters, strings can.
- Values of other registry types, like `REG_EXPAND_SZ`, return an error.

### Linux

- Operations are serialized across handles in one process, concurrent writers in different processes are
  unsupported.
- Corrupt JSON returns an error and is preserved.
