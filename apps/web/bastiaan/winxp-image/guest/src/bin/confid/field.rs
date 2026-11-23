/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Arithmetic in the prime field GF(p) the activation curve is defined over.

use std::ops::{Add, Mul, Neg, Sub};

/// The 57-bit field prime
pub const P: u64 = 0x016A_6B03_6D7F_2A79;

/// p - 1 = Q * 2^S with Q odd, for Tonelli-Shanks
const S: u32 = (P - 1).trailing_zeros();
const Q: u64 = (P - 1) >> S;

/// An element of GF(p), always kept reduced to 0..p
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fp(u64);

impl Fp {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);
    /// The smallest quadratic non-residue
    pub const NON_RESIDUE: Self = Self(43);

    pub const fn new(value: u64) -> Self {
        Self(value % P)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn is_odd(self) -> bool {
        self.0 & 1 == 1
    }

    pub fn pow(self, mut exponent: u64) -> Self {
        let (mut base, mut result) = (self, Self::ONE);
        while exponent != 0 {
            if exponent & 1 == 1 {
                result = result * base;
            }
            base = base * base;
            exponent >>= 1;
        }
        result
    }

    /// The multiplicative inverse via Fermat's little theorem, zero maps to zero
    pub fn inv(self) -> Self {
        self.pow(P - 2)
    }

    /// A square root via Tonelli-Shanks, or `None` when the element is a non-residue
    pub fn sqrt(self) -> Option<Self> {
        if self == Self::ZERO {
            return Some(self);
        }
        if self.pow((P - 1) / 2) != Self::ONE {
            return None;
        }
        let mut order = S;
        let mut c = Self::NON_RESIDUE.pow(Q);
        let mut t = self.pow(Q);
        let mut root = self.pow(Q.div_ceil(2));
        while t != Self::ONE {
            let mut i = 0;
            let mut t2 = t;
            while t2 != Self::ONE {
                t2 = t2 * t2;
                i += 1;
            }
            let b = c.pow(1 << (order - i - 1));
            c = b * b;
            order = i;
            root = root * b;
            t = t * c;
        }
        Some(root)
    }
}

impl Add for Fp {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.0 + rhs.0)
    }
}

impl Sub for Fp {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self + -rhs
    }
}

impl Neg for Fp {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(P - self.0)
    }
}

impl Mul for Fp {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self((self.0 as u128 * rhs.0 as u128 % P as u128) as u64)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_and_sqrt() {
        let x = Fp::new(0x0123_4567_89AB_CDEF);
        assert_eq!(x * x.inv(), Fp::ONE);
        let root = (x * x).sqrt().map(|r| r * r);
        assert_eq!(root, Some(x * x));
        assert_eq!(Fp::NON_RESIDUE.sqrt(), None);
    }
}
