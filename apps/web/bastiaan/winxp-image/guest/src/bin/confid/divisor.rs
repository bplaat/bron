/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Reduced divisors in Mumford representation on the genus 2 curve y^2 = f(x) over GF(p), with
//! the group law from Cantor's algorithm.

use crate::field::Fp;
use crate::poly::Poly;

/// The coefficients of the monic quintic f, from low to high degree
const CURVE: [u64; 6] = [
    0,
    0x0021_8401_36C8_5381,
    0x0044_197B_8389_2AD0,
    0x0140_0606_322B_3B04,
    0x0140_0606_322B_3B04,
    1,
];

fn curve() -> Poly {
    Poly::new(CURVE.iter().map(|&c| Fp::new(c)).collect())
}

/// A reduced divisor (u, v): u monic with degree at most 2, deg v < deg u and u | v^2 - f
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Divisor {
    pub u: Poly,
    pub v: Poly,
}

impl Divisor {
    pub fn zero() -> Self {
        Self {
            u: Poly::one(),
            v: Poly::default(),
        }
    }

    /// Completes the monic quadratic u = x^2 + u1*x + u0 to a divisor by finding a v = v1*x + v0
    /// with v^2 = f (mod u), if there is one
    pub fn from_quadratic(u0: Fp, u1: Fp) -> Option<Self> {
        let u = Poly::new(vec![u0, u1, Fp::ONE]);
        let r = curve().rem(&u);
        let (r0, r1) = (r.coeff(0), r.coeff(1));
        // Comparing coefficients with x^2 = -u1*x - u0 gives 2*v0*v1 - u1*v1^2 = r1 and
        // v0^2 - u0*v1^2 = r0, eliminating v0 leaves a quadratic in t = v1^2:
        // (u1^2 - 4*u0)*t^2 + 2*(r1*u1 - 2*r0)*t + r1^2 = 0
        let two = Fp::new(2);
        let a = u1 * u1 - two * two * u0;
        let half_b = r1 * u1 - two * r0;
        let root = (half_b * half_b - a * r1 * r1).sqrt()?;
        let v1 = [root, -root]
            .into_iter()
            .find_map(|root| ((root - half_b) * a.inv()).sqrt())?;
        let v0 = (r1 + u1 * v1 * v1) * (two * v1).inv();
        Some(Self {
            u,
            v: Poly::new(vec![v0, v1]),
        })
    }

    /// Adds two divisors with Cantor's algorithm
    pub fn add(&self, other: &Self) -> Self {
        let f = curve();
        // Composition
        let (d1, e1, e2) = Poly::xgcd(&self.u, &other.u);
        let (d, c1, c2) = Poly::xgcd(&d1, &(self.v.clone() + &other.v));
        let mut u = (self.u.clone() * &other.u).div_rem(&(d.clone() * &d)).0;
        let mut v = (c1.clone() * &e1 * &self.u * &other.v
            + &(c1 * &e2 * &other.u * &self.v)
            + &(c2 * &(self.v.clone() * &other.v + &f)))
            .div_rem(&d)
            .0
            .rem(&u);
        // Reduction
        while u.degree() > 2 {
            u = (f.clone() - &(v.clone() * &v)).div_rem(&u).0;
            v = (-v).rem(&u);
        }
        let u = u.monic();
        Self { v: v.rem(&u), u }
    }

    /// Multiplies the divisor by a scalar with double-and-add
    pub fn mul(&self, scalar: u128) -> Self {
        (0..u128::BITS - scalar.leading_zeros())
            .rev()
            .fold(Self::zero(), |acc, bit| {
                let doubled = acc.add(&acc);
                if scalar >> bit & 1 == 1 {
                    doubled.add(self)
                } else {
                    doubled
                }
            })
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_law() {
        let d = Divisor::from_quadratic(Fp::new(5), Fp::new(7))
            .or_else(|| Divisor::from_quadratic(Fp::new(6), Fp::new(7)))
            .expect("one of the quadratics has a divisor");
        let f = curve();
        assert!((f - &(d.v.clone() * &d.v)).rem(&d.u).is_zero());
        assert_eq!(d.mul(3), d.add(&d).add(&d));
        assert_eq!(d.mul(5).add(&d.mul(7)), d.mul(12));
        let neg = Divisor {
            u: d.u.clone(),
            v: -d.v.clone(),
        };
        assert_eq!(d.add(&neg), Divisor::zero());
    }
}
