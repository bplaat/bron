# dprint-toml

The repository's dprint configuration uses this plugin for `.toml` files. It is a fork of the
[dprint TOML plugin](https://github.com/dprint/dprint-plugin-toml) 0.10.0 with these
additional `Cargo.toml` conventions:

- Put the tables in a fixed order: `package`, `workspace`, `lib`, `lints`, `features`, the dependencies each
  followed by their target specific ones, `[[bin]]`, `[[example]]`, `[[test]]`, `[[bench]]`, `patch` and `profile`.
  A file with a table header key that contains an escape keeps its table order.
- Sort the entries of every dependencies table, including `build-dependencies`, `workspace.dependencies` and
  the target specific ones
- Order the `package` and `workspace.package` keys as `name`, `version`, the others alphabetically and
  `description` last
- Sort the `workspace` `members` array

It also keeps a blank line between an empty table and the comment introducing the next table.

Range formatting expands the selection to complete entries, table headers, and comments. It may
include the containing table when needed for consistent indentation. Cargo table ordering applies
during whole-file formatting.

The formatting checks and VS Code setup build the plugin into `target/`. To build it before running
dprint directly, run these commands from the workspace root.

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --locked -p dprint-toml --target wasm32-unknown-unknown --target-dir target
```
