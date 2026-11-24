# dprint-toml

The repository's dprint configuration uses this plugin for `.toml` files. It is a fork of the
[dprint TOML plugin](https://github.com/dprint/dprint-plugin-toml) 0.10.0 without range formatting, whose
`Cargo.toml` conventions are changed to:

- Put the tables in a fixed order: `package`, `workspace`, `lib`, `lints`, `features`, the dependencies each
  followed by their target specific ones, `[[bin]]`, `[[example]]`, `[[test]]`, `[[bench]]`, `patch` and `profile`
- Sort the entries of every dependencies table, including `build-dependencies` and the target specific ones
- Keep the entries of `[package]` in the order they were written in

It also keeps a blank line between an empty table and the comment introducing the next table.

The formatting checks and VS Code setup build the plugin into `target/`. To build it before running
dprint directly, run these commands from the workspace root.

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --locked -p dprint-toml --target wasm32-unknown-unknown --target-dir target
```
