# ZIP

A small ZIP reader, append helper, and `unzip` command. Stored and Deflate entries
are supported, including signed and unsigned data descriptors. The reader validates
record bounds, entry sizes, extra fields, and CRCs. ZIP64, split archives, and
encryption are unsupported.

```sh
cargo run -p zip --bin unzip -- -l archive.zip
cargo run -p zip --bin unzip -- archive.zip -d output
```

Extraction refuses to overwrite existing files. It restores modification times
from Unix or NTFS extra fields, falling back to DOS timestamps interpreted in the
local time zone. On Unix it restores ordinary permission bits, including executable
bits, and creates relative symlinks whose targets remain inside the destination.
Directory permissions and times are applied after their children are extracted.
Unsafe paths, escaping or cyclic symlinks, and special files are rejected. Symlink
extraction on other operating systems reports an unsupported-feature error.
Ownership, setuid/setgid/sticky bits, ACLs, and extended attributes are not restored.

The library exposes `unix_mode()`, `is_file()`, `is_dir()`, `is_symlink()`, and
`last_modified()` on entries. `append_file()` preserves existing archive entries
and metadata while appending one regular file, `append_stored_file()` does the same
without compression. Zero padding in extra fields, which aapt2 and zipalign add for
alignment, is accepted.

Python-generated interoperability fixtures and their regeneration script live in
`tests/fixtures/`. Running the Rust tests does not require Python.
