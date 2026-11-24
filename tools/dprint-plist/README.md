# dprint-plist

The repository's dprint configuration uses this plugin for `.plist` and `.entitlements`. It delegates the
contents to the markup formatter while keeping the value directly under `<plist>` unindented and always
writing self-closing tags without a space, such as `<true/>`, because some macOS command line tools reject
`<true />`.

Range formatting applies the markup formatter's whitespace edits inside the selected text. Binary
property lists are skipped.

The formatting checks and VS Code setup build the plugin into `target/`. To build it before running
dprint directly, run these commands from the workspace root.

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --locked -p dprint-plist --target wasm32-unknown-unknown --target-dir target
```
