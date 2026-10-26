# Preferences

Minimal shared preferences for desktop and mobile, using native value types.

| Platform        | Backend                                       | Dependencies                      |
| --------------- | --------------------------------------------- | --------------------------------- |
| Android         | Native SharedPreferences through JNI          | Local `jni`                       |
| iOS / macOS     | Native NSUserDefaults suite                   | Local `objc2`                     |
| Windows / Linux | JSON file in the application config directory | Local `directories`, `serde_json` |

```rust,no_run
use preferences::{Preferences, Value};

# fn main() -> std::io::Result<()> {
let prefs = Preferences::new("nl.bplaat.example")?;
prefs.set("dark_mode", true)?;
prefs.set("launches", 42_i64)?;
prefs.set("volume", 0.75_f32)?;
prefs.set("username", "Bastiaan")?;
assert_eq!(prefs.get("dark_mode")?, Some(Value::Bool(true)));
prefs.remove("username")?;
# Ok(())
# }
```

Values are `bool`, `i64`, finite `f32`, or `String`. Floats use Android's native
32-bit precision. `get` returns `None` for missing keys and an error for unsupported
native values. Keys and strings support Unicode and embedded NUL characters.

On Android the Java VM must be registered through `jni::vm::JavaVM::from_raw` first
(the local `bwindow` does this already). The NDK exposes no Context, so the store
gets the running Application from the non-SDK `ActivityThread.currentApplication()`,
which is on the unsupported list and allowed for every target SDK.

Application IDs are nonempty ASCII letters, digits, dots, underscores and hyphens,
excluding `.` and `..`. Use a reverse-DNS ID such as `nl.bplaat.example`.
Apple uses that ID as its NSUserDefaults suite, or the standard defaults when it is
the app's own bundle ID, and reads only that domain, ignoring command-line
arguments and registered or global defaults. Windows and
Linux store `preferences.json` in
`directories::ProjectDirs::from_path(id).config_dir()`.
To preserve embedded NUL characters across Apple process restarts, those strings
use UTF-8 NSData and keys use an escaped `__preferences__` prefix. Keys starting
with that prefix are escaped too, so distinct keys cannot collide.

Android writes use synchronous `commit` and report persistence failures. Apple
writes follow NSUserDefaults' asynchronous persistence. JSON writes read the
current file, synchronize a temporary file and atomically replace the destination.
On Linux, new directory entries and the replacement are synchronized as well.
File operations are serialized across handles in one process; concurrent writers
in different processes are unsupported. Corrupt JSON returns an error and is
preserved. This store is intended for small preferences, not secrets.
