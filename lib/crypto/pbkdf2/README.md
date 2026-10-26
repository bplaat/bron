# PBKDF2

A small PBKDF2-HMAC-SHA256 password hashing library.

## Example

```rs
let hash = pbkdf2::password_hash("my_secure_password");

assert!(pbkdf2::password_verify("my_secure_password", &hash).unwrap());
assert!(!pbkdf2::password_verify("wrong_password", &hash).unwrap());
```

## Features

Disable default features for `no_std` support with `alloc`. Automatic salt generation requires `std`.

- Generates random salts for password hashes
- Encodes password hashes using the PHC string format
- Verifies hashes using constant-time comparison
- Provides a low-level PBKDF2-HMAC-SHA256 key derivation function

## License

Copyright © 2024-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
