/*
 * Copyright (c) 2024 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::error::Error;
use core::fmt::{self, Display, Formatter};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD as BASE64_NO_PAD;
use subtle::ConstantTimeEq as _;

use crate::pbkdf2_hmac_sha256;

/// Default recommended safe amount of iterations for PBKDF2-HMAC-SHA256 (https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#pbkdf2)
pub const DEFAULT_SAFE_ITERATIONS: u32 = 600_000;
/// Maximum accepted work factor when verifying a password hash.
pub const MAX_PASSWORD_HASH_ITERATIONS: u32 = 1_000_000;

const MIN_SALT_LENGTH: usize = 8;
const MAX_SALT_LENGTH: usize = 64;
const HASH_LENGTH: usize = 32;

/// Hash password using PBKDF2-HMAC-SHA256 returns string in PHC standard (https://github.com/P-H-C/phc-string-format/blob/master/phc-sf-spec.md)
#[cfg(feature = "std")]
pub fn password_hash(password: &str) -> String {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("Can't get random bytes");
    password_hash_customized(password, &salt, DEFAULT_SAFE_ITERATIONS)
}

/// Hash password with custom salt and iterations using PBKDF2-HMAC-SHA256 returns string in PHC standard (https://github.com/P-H-C/phc-string-format/blob/master/phc-sf-spec.md)
pub fn password_hash_customized(password: &str, salt: &[u8], iterations: u32) -> String {
    assert!(
        (1..=MAX_PASSWORD_HASH_ITERATIONS).contains(&iterations),
        "password hash iterations must be between 1 and {MAX_PASSWORD_HASH_ITERATIONS}"
    );
    assert!(
        (MIN_SALT_LENGTH..=MAX_SALT_LENGTH).contains(&salt.len()),
        "password hash salt must be between {MIN_SALT_LENGTH} and {MAX_SALT_LENGTH} bytes"
    );
    let hashed_password = pbkdf2_hmac_sha256(password.as_bytes(), salt, iterations, HASH_LENGTH);
    format!(
        "$pbkdf2-sha256$t={}${}${}",
        iterations,
        BASE64_NO_PAD.encode(salt),
        BASE64_NO_PAD.encode(&hashed_password)
    )
}

/// Verify password using hash string in PHC standard
pub fn password_verify(password: &str, hash: &str) -> Result<bool, PasswordHashDecodeError> {
    let parts = hash.split('$').collect::<Vec<_>>();
    let ["", "pbkdf2-sha256", parameters, encoded_salt, encoded_hash] = parts.as_slice() else {
        return Err(PasswordHashDecodeError);
    };
    let iterations = parameters
        .strip_prefix("t=")
        .ok_or(PasswordHashDecodeError)?
        .parse::<u32>()
        .map_err(|_| PasswordHashDecodeError)?;
    if !(1..=MAX_PASSWORD_HASH_ITERATIONS).contains(&iterations) {
        return Err(PasswordHashDecodeError);
    }
    let salt = BASE64_NO_PAD
        .decode(encoded_salt)
        .map_err(|_| PasswordHashDecodeError)?;
    if !(MIN_SALT_LENGTH..=MAX_SALT_LENGTH).contains(&salt.len()) {
        return Err(PasswordHashDecodeError);
    }
    let stored_hash = BASE64_NO_PAD
        .decode(encoded_hash)
        .map_err(|_| PasswordHashDecodeError)?;
    if stored_hash.len() != HASH_LENGTH {
        return Err(PasswordHashDecodeError);
    }
    let computed_hash = pbkdf2_hmac_sha256(password.as_bytes(), &salt, iterations, HASH_LENGTH);
    Ok(bool::from(stored_hash.ct_eq(&computed_hash)))
}

// MARK: PasswordHashDecodeError
/// Password hash decode error
#[derive(Debug)]
pub struct PasswordHashDecodeError;

impl Display for PasswordHashDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Password hash decode error")
    }
}

impl Error for PasswordHashDecodeError {}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[cfg(feature = "std")]
    fn test_hash_and_verify_password() {
        let hashed = password_hash("my_secure_password");
        assert!(password_verify("my_secure_password", &hashed).unwrap());
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_verify_incorrect_password() {
        let hashed = password_hash("my_secure_password");
        assert!(!password_verify("wrong_password", &hashed).unwrap());
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_hash_is_different_for_same_password() {
        let hashed1 = password_hash("my_secure_password");
        let hashed2 = password_hash("my_secure_password");
        assert_ne!(hashed1, hashed2);
    }

    #[test]
    fn test_verify_password_with_invalid_parts() {
        assert!(
            password_verify(
                "password",
                "$pbkdf2-sha256$t=100000$invalid*salt$inval&idhash"
            )
            .is_err()
        );
    }

    #[test]
    fn test_verify_rejects_invalid_hash_metadata() {
        let valid_salt = BASE64_NO_PAD.encode([0; 16]);
        let valid_hash = BASE64_NO_PAD.encode([0; HASH_LENGTH]);
        for hash in [
            format!("$argon2$t=1${valid_salt}${valid_hash}"),
            format!("$pbkdf2-sha256$i=1${valid_salt}${valid_hash}"),
            format!("$pbkdf2-sha256$t=0${valid_salt}${valid_hash}"),
            format!(
                "$pbkdf2-sha256$t={}${valid_salt}${valid_hash}",
                MAX_PASSWORD_HASH_ITERATIONS + 1
            ),
            format!("$pbkdf2-sha256$t=1${valid_salt}${valid_hash}$extra"),
            format!("$pbkdf2-sha256$t=1$AA${valid_hash}"),
            format!("$pbkdf2-sha256$t=1${valid_salt}$AA"),
        ] {
            assert!(password_verify("password", &hash).is_err(), "hash: {hash}");
        }
    }

    #[test]
    #[should_panic(expected = "PBKDF2 iterations must be greater than zero")]
    fn test_pbkdf2_rejects_zero_iterations() {
        pbkdf2_hmac_sha256(b"password", b"salt", 0, HASH_LENGTH);
    }
}
