/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Polynomials over GF(p).

use std::ops::{Add, Mul, Neg, Sub};

use crate::field::Fp;

/// A polynomial with its coefficients from low to high degree, without trailing zeros
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Poly(Vec<Fp>);

impl Poly {
    pub fn new(mut coeffs: Vec<Fp>) -> Self {
        while coeffs.last() == Some(&Fp::ZERO) {
            coeffs.pop();
        }
        Self(coeffs)
    }

    pub fn one() -> Self {
        Self(vec![Fp::ONE])
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    /// The degree, where the zero polynomial counts as degree 0
    pub fn degree(&self) -> usize {
        self.0.len().saturating_sub(1)
    }

    pub fn coeff(&self, index: usize) -> Fp {
        self.0.get(index).copied().unwrap_or_default()
    }

    fn leading(&self) -> Fp {
        self.0.last().copied().unwrap_or_default()
    }

    fn scale(self, factor: Fp) -> Self {
        Self::new(self.0.into_iter().map(|c| c * factor).collect())
    }

    /// Scales the polynomial to leading coefficient one
    pub fn monic(self) -> Self {
        let factor = self.leading().inv();
        self.scale(factor)
    }

    /// Long division by a nonzero divisor, returns the quotient and remainder
    pub fn div_rem(&self, divisor: &Self) -> (Self, Self) {
        let n = divisor.0.len();
        let lead_inv = divisor.leading().inv();
        let mut rem = self.0.clone();
        let mut quot = vec![Fp::ZERO; (rem.len() + 1).saturating_sub(n)];
        for i in (0..quot.len()).rev() {
            let factor = rem[i + n - 1] * lead_inv;
            quot[i] = factor;
            for (r, &d) in rem[i..i + n].iter_mut().zip(&divisor.0) {
                *r = *r - factor * d;
            }
        }
        (Self::new(quot), Self::new(rem))
    }

    pub fn rem(&self, divisor: &Self) -> Self {
        self.div_rem(divisor).1
    }

    /// The extended Euclidean algorithm: returns the monic gcd g with s and t so that s*a + t*b = g
    pub fn xgcd(a: &Self, b: &Self) -> (Self, Self, Self) {
        let (mut r0, mut r1) = (a.clone(), b.clone());
        let (mut s0, mut s1) = (Self::one(), Self::default());
        let (mut t0, mut t1) = (Self::default(), Self::one());
        while !r1.is_zero() {
            let (q, r) = r0.div_rem(&r1);
            r0 = std::mem::replace(&mut r1, r);
            let s = s0 - &(q.clone() * &s1);
            s0 = std::mem::replace(&mut s1, s);
            let t = t0 - &(q * &t1);
            t0 = std::mem::replace(&mut t1, t);
        }
        let factor = r0.leading().inv();
        (r0.scale(factor), s0.scale(factor), t0.scale(factor))
    }
}

impl Add<&Poly> for Poly {
    type Output = Self;
    fn add(self, rhs: &Poly) -> Self {
        let len = self.0.len().max(rhs.0.len());
        Self::new((0..len).map(|i| self.coeff(i) + rhs.coeff(i)).collect())
    }
}

impl Sub<&Poly> for Poly {
    type Output = Self;
    fn sub(self, rhs: &Poly) -> Self {
        self + &-rhs.clone()
    }
}

impl Neg for Poly {
    type Output = Self;
    fn neg(self) -> Self {
        Self(self.0.into_iter().map(|c| -c).collect())
    }
}

impl Mul<&Poly> for Poly {
    type Output = Self;
    fn mul(self, rhs: &Poly) -> Self {
        if self.is_zero() || rhs.is_zero() {
            return Self::default();
        }
        let mut product = vec![Fp::ZERO; self.0.len() + rhs.0.len() - 1];
        for (i, &a) in self.0.iter().enumerate() {
            for (j, &b) in rhs.0.iter().enumerate() {
                product[i + j] = product[i + j] + a * b;
            }
        }
        Self::new(product)
    }
}
