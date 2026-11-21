/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! confid - generates the confirmation ID that activates Windows XP offline from its installation
//! ID. Usage: `confid <installation id>`, prints the confirmation ID, on failure prints the error
//! status to stderr and exits with it.
//!
//! The decrypted installation ID seeds a divisor on a genus 2 hyperelliptic curve, the
//! confirmation ID encodes that divisor multiplied by the private activation scalar.

mod cipher;
mod divisor;
mod field;
mod installation_id;
mod poly;

use std::process::ExitCode;

use crate::cipher::feistel;
use crate::divisor::Divisor;
use crate::field::{Fp, P};
use crate::installation_id::{InstallationId, le_int};

/// The private scalar that maps a seed divisor to the confirmation ID divisor
const SCALAR: u128 = 0x40DA_7C36_D44C_04E2_1B9D_10F1_27C1;

/// A generation failure, the discriminant is the exit status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    TooShort = 1,
    TooLarge = 2,
    InvalidCharacter = 3,
    InvalidCheckDigit = 4,
    UnknownVersion = 5,
    Unlucky = 6,
}

/// Derives the seed divisor: hashes the hardware and product ID with an attempt counter into an
/// x^2 + u1*x + u0 without roots in GF(p) until one has a matching v
fn seed_divisor(id: &InstallationId) -> Result<Divisor, Error> {
    let [a, b, c, d] = id.product_id;
    let mut key = [0; 16];
    key[..8].copy_from_slice(&id.hardware_id.to_le_bytes());
    key[8..].copy_from_slice(&(a << 41 | b << 58 | c << 17 | d).to_le_bytes());
    (0..=0x80)
        .find_map(|attempt| {
            let mut block = [0; 14];
            block[7] = attempt;
            feistel(&mut block, &key, true);
            let n = le_int(&block);
            // u = (x + x1)^2 - NON_RESIDUE * x2^2 with x2 != 0
            let x1 = Fp::new((n % u128::from(P)) as u64);
            let x2 = Fp::new((n / u128::from(P)) as u64 + 1);
            Divisor::from_quadratic(x1 * x1 - Fp::NON_RESIDUE * x2 * x2, x1 + x1)
        })
        .ok_or(Error::Unlucky)
}

/// Encodes a divisor as a number below 10^35
fn encode(divisor: &Divisor) -> u128 {
    let p = u128::from(P);
    let (u, v) = (&divisor.u, &divisor.v);
    let base = |hi: u64, lo: u64| (p + 1) * u128::from(hi) + u128::from(lo);
    match u.degree() {
        0 => p * (p + 2),
        1 => base(u.coeff(0).value(), P),
        _ => {
            // u = (x + x1)^2 - s, with roots -x1 +- sqrt(s)
            let x1 = u.coeff(1) * Fp::new(2).inv();
            let s = x1 * x1 - u.coeff(0);
            match s.sqrt() {
                None => {
                    let x2 = (s * Fp::NON_RESIDUE.inv()).sqrt().unwrap_or_default();
                    base(P + x2.value(), x1.value())
                }
                Some(x2) => {
                    let (r1, r2) = (x1 - x2, x1 + x2);
                    let y = |r: Fp| v.coeff(0) - v.coeff(1) * r;
                    let (lo, hi) = (r1.value().min(r2.value()), r1.value().max(r2.value()));
                    if y(r1).is_odd() != y(r2).is_odd() {
                        base(hi, lo)
                    } else {
                        base(lo, hi)
                    }
                }
            }
        }
    }
}

/// Formats the number as seven groups of five digits, each followed by a check digit
fn format_confirmation_id(number: u128) -> String {
    format!("{number:035}")
        .as_bytes()
        .chunks(5)
        .map(|group| {
            let sum: u32 = group
                .iter()
                .zip([1, 2, 1, 2, 1])
                .map(|(&c, weight)| u32::from(c - b'0') * weight)
                .sum();
            let mut text: String = group.iter().map(|&c| char::from(c)).collect();
            text.push(char::from(b'0' + (sum % 7) as u8));
            text
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// Generates the confirmation ID for an installation ID
fn confirmation_id(installation_id: &str) -> Result<String, Error> {
    let id = installation_id::decode(installation_id)?;
    let divisor = seed_divisor(&id)?.mul(SCALAR);
    Ok(format_confirmation_id(encode(&divisor)))
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [installation_id] = args.as_slice() else {
        return ExitCode::from(2);
    };
    match confirmation_id(&installation_id.to_string_lossy()) {
        Ok(confirmation_id) => {
            println!("{confirmation_id}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Generation error: {}", err as u8);
            ExitCode::from(err as u8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "045466-405094-827133-595744-139473-449011-525423-446500-428590";

    #[test]
    fn valid_installation_ids() {
        for (installation_id, expected) in [
            (VALID, "103810-496055-248085-847094-481835-981786-914450"),
            (
                "211122-182512-158133-045776-397792-742612-896993-399564-318193",
                "116896-463166-513281-796211-982893-866662-687862",
            ),
            (
                "205540-870253-644004-701606-249100-235882-174892-378245-913310",
                "155312-247784-967412-909752-311481-997940-767326",
            ),
            (
                "141200-612430-047494-589254-352114-762365-911705-245450-699005",
                "096302-597553-871484-090106-269382-162001-458986",
            ),
            (
                "278725-340613-174326-625151-569213-865602-791881-925132-820432",
                "165293-231356-936755-961231-074891-986131-959150",
            ),
            (
                "836422-329531-132393-206873-209945-876664-376670-678616-92",
                "109105-505944-742714-484886-178280-632471-624423",
            ),
            (
                "082592-180725-047953-825452-759571-118511-434996-784973-66",
                "041896-995650-445474-002746-763793-958444-128791",
            ),
        ] {
            assert_eq!(confirmation_id(installation_id).as_deref(), Ok(expected));
        }
    }

    #[test]
    fn separators_are_ignored() {
        let expected = confirmation_id(VALID);
        assert_eq!(confirmation_id(&VALID.replace('-', "")), expected);
        assert_eq!(confirmation_id(&VALID.replace('-', " ")), expected);
    }

    #[test]
    fn invalid_installation_ids() {
        for (installation_id, error) in [
            ("", Error::TooShort),
            (&VALID[..VALID.len() - 7], Error::TooShort),
            (&format!("{VALID}-1"), Error::TooLarge),
            (&VALID.replacen('7', "x", 1), Error::InvalidCharacter),
            (
                &format!("{}1", &VALID[..VALID.len() - 1]),
                Error::InvalidCheckDigit,
            ),
            (
                "301282916142336140364501306061720362190192335011280810",
                Error::UnknownVersion,
            ),
        ] {
            assert_eq!(confirmation_id(installation_id), Err(error));
        }
    }
}
