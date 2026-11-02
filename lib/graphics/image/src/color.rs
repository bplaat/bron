/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Color management: decoders describe their samples with an ICC profile, CICP code points or
// chromaticities and gamma, and a `Transform` converts them to sRGB. Profiles that cannot be
// used are ignored, so their samples are treated as sRGB.

// Codecs share this module, so each feature subset leaves some of it unused.
#![cfg_attr(
    not(all(feature = "bmp", feature = "jpeg", feature = "png", feature = "qoi")),
    allow(dead_code)
)]

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::math::{exp, pow};

type Matrix = [[f64; 3]; 3];

/// The device color model of the samples a profile describes.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum ColorModel {
    Gray,
    Rgb,
    Cmyk,
}

impl ColorModel {
    const fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }
}

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    core::array::from_fn(|i| core::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: &Matrix, v: [f64; 3]) -> [f64; 3] {
    core::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn invert(m: &Matrix) -> Option<Matrix> {
    let cofactor = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    };
    let det = (0..3).map(|c| m[0][c] * cofactor(0, c)).sum::<f64>();
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    Some(core::array::from_fn(|i| {
        core::array::from_fn(|j| cofactor(j, i) / det)
    }))
}

const fn diagonal(v: [f64; 3]) -> Matrix {
    [[v[0], 0.0, 0.0], [0.0, v[1], 0.0], [0.0, 0.0, v[2]]]
}

// MARK: Primaries
/// The CIE xy chromaticities of the white point and the red, green and blue primaries.
pub(crate) type Primaries = [[f64; 2]; 4];

const D65: [f64; 2] = [0.3127, 0.3290];
pub(crate) const SRGB: Primaries = [D65, [0.64, 0.33], [0.30, 0.60], [0.15, 0.06]];
pub(crate) const ADOBE_RGB: Primaries = [D65, [0.64, 0.33], [0.21, 0.71], [0.15, 0.06]];
const DISPLAY_P3: Primaries = [D65, [0.680, 0.320], [0.265, 0.690], [0.150, 0.060]];
const DCI_P3: Primaries = [
    [0.314, 0.351],
    [0.680, 0.320],
    [0.265, 0.690],
    [0.150, 0.060],
];
const BT2020: Primaries = [D65, [0.708, 0.292], [0.170, 0.797], [0.131, 0.046]];
const BT470M: Primaries = [[0.310, 0.316], [0.67, 0.33], [0.21, 0.71], [0.14, 0.08]];
const BT470BG: Primaries = [D65, [0.64, 0.33], [0.29, 0.60], [0.15, 0.06]];
const SMPTE170M: Primaries = [D65, [0.630, 0.340], [0.310, 0.595], [0.155, 0.070]];
const EBU3213: Primaries = [D65, [0.630, 0.340], [0.295, 0.605], [0.155, 0.077]];

/// The ICC profile connection space white, D50.
const D50_XYZ: [f64; 3] = [0.9642, 1.0, 0.8249];

const fn xy_to_xyz([x, y]: [f64; 2]) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// Returns the matrix from linear RGB to D50-adapted XYZ, like ICC matrix profiles store.
fn rgb_to_xyz_d50(primaries: &Primaries) -> Option<Matrix> {
    if primaries
        .iter()
        .any(|[x, y]| !(0.0..=1.0).contains(x) || !(*y > 0.0 && *y <= 1.0))
    {
        return None;
    }
    let [r, g, b] = [1, 2, 3].map(|i| xy_to_xyz(primaries[i]));
    let columns = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let white = xy_to_xyz(primaries[0]);
    let scale = apply(&invert(&columns)?, white);
    let to_xyz = multiply(&columns, &diagonal(scale));
    // Bradford chromatic adaptation from the white point to D50.
    const BRADFORD: Matrix = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let source = apply(&BRADFORD, white);
    let target = apply(&BRADFORD, D50_XYZ);
    let adapt = multiply(
        &invert(&BRADFORD)?,
        &multiply(
            &diagonal(core::array::from_fn(|i| target[i] / source[i])),
            &BRADFORD,
        ),
    );
    Some(multiply(&adapt, &to_xyz))
}

/// Returns the matrix from D50 XYZ to linear sRGB.
fn xyz_d50_to_srgb() -> Matrix {
    invert(&rgb_to_xyz_d50(&SRGB).expect("sRGB primaries")).expect("sRGB matrix")
}

// MARK: Curves
/// A transfer function from encoded samples to linear light, both normalized to [0, 1].
#[derive(Clone, PartialEq)]
pub(crate) enum Curve {
    /// ICC parametric curve `(a x + b)^g + e` for `x >= d`, otherwise `c x + f`, as
    /// `[g, a, b, c, d, e, f]`.
    Parametric([f64; 7]),
    /// Evenly spaced samples, interpolated linearly.
    Table(Vec<f32>),
    /// SMPTE ST 2084, with 203 cd/m² as SDR white.
    Pq,
    /// ARIB STD-B67 hybrid log-gamma, with 75% signal as SDR white.
    Hlg,
}

impl Curve {
    pub(crate) const SRGB: Self = Self::Parametric([
        2.4,
        1.0 / 1.055,
        0.055 / 1.055,
        1.0 / 12.92,
        0.04045,
        0.0,
        0.0,
    ]);
    pub(crate) const LINEAR: Self = Self::gamma(1.0);

    pub(crate) const fn gamma(gamma: f64) -> Self {
        Self::Parametric([gamma, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    }

    fn eval(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Parametric([g, a, b, c, d, e, f]) => {
                if x >= *d {
                    pow(a * x + b, *g) + e
                } else {
                    c * x + f
                }
            }
            Self::Table(values) => f64::from(Table::lerp(values, x as f32)),
            Self::Pq => {
                const M1: f64 = 2610.0 / 16384.0;
                const M2: f64 = 2523.0 / 4096.0 * 128.0;
                const C1: f64 = 3424.0 / 4096.0;
                const C2: f64 = 2413.0 / 4096.0 * 32.0;
                const C3: f64 = 2392.0 / 4096.0 * 32.0;
                let p = pow(x, 1.0 / M2);
                pow((p - C1).max(0.0) / (C2 - C3 * p), 1.0 / M1) * 10000.0 / 203.0
            }
            Self::Hlg => {
                fn scene(x: f64) -> f64 {
                    const A: f64 = 0.178_832_77;
                    const B: f64 = 0.284_668_92;
                    const C: f64 = 0.559_910_73;
                    if x <= 0.5 {
                        x * x / 3.0
                    } else {
                        (exp((x - C) / A) + B) / 12.0
                    }
                }
                scene(x) / scene(0.75)
            }
        }
    }

    fn table(&self) -> Table {
        match self {
            Self::Table(values) => Table(values.clone()),
            _ if *self == Self::LINEAR => Table(alloc::vec![0.0, 1.0]),
            _ => Table::sample(|x| self.eval(x)),
        }
    }

    // Parses a `curv` or `para` element and returns it with its padded length.
    fn parse(data: &[u8]) -> Option<(Self, usize)> {
        match data.get(..4)? {
            b"curv" => {
                let count = be32(data, 8)? as usize;
                let len = 12 + count.checked_mul(2)?;
                let curve = match count {
                    0 => Self::LINEAR,
                    1 => Self::gamma(f64::from(be16(data, 12)?) / 256.0),
                    _ => Self::Table(
                        data.get(12..len)?
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|v| f32::from(u16::from_be_bytes(*v)) / 65535.0)
                            .collect(),
                    ),
                };
                Some((curve, len.next_multiple_of(4)))
            }
            b"para" => {
                let kind = be16(data, 8)?;
                let count = *[1, 3, 4, 5, 7].get(usize::from(kind))?;
                let mut p = [0.0; 7];
                for (i, value) in p.iter_mut().take(count).enumerate() {
                    *value = s15_16(data, 12 + i * 4)?;
                }
                let [g, a, b, c, ..] = p;
                let params = match kind {
                    0 => [g, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    1 if a != 0.0 => [g, a, b, 0.0, -b / a, 0.0, 0.0],
                    2 if a != 0.0 => [g, a, b, 0.0, -b / a, c, c],
                    3 | 4 => p,
                    _ => return None,
                };
                Some((Self::Parametric(params), 12 + count * 4))
            }
            _ => None,
        }
    }
}

// MARK: Tables
include!(concat!(env!("OUT_DIR"), "/srgb.rs"));

#[cfg(test)]
fn srgb_encode(x: f64) -> f64 {
    if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * pow(x, 1.0 / 2.4) - 0.055
    }
}

/// Converts an 8-bit sRGB sample to linear light in [0, 1], for example to average pixels
/// when resizing or building mipmaps.
pub fn srgb_to_linear(value: u8) -> f32 {
    SRGB_TO_LINEAR[usize::from(value)]
}

/// Converts linear light to the nearest 8-bit sRGB sample, clamping to [0, 1].
pub fn linear_to_srgb(value: f32) -> u8 {
    SRGB_ROUNDING.partition_point(|threshold| *threshold <= value) as u8
}

/// A function on [0, 1] sampled at evenly spaced points.
#[derive(Clone)]
struct Table(Vec<f32>);

impl Table {
    fn sample(f: impl Fn(f64) -> f64) -> Self {
        Self(
            (0..=TABLE_STEPS)
                .map(|i| f(i as f64 / TABLE_STEPS as f64) as f32)
                .collect(),
        )
    }

    fn eval(&self, x: f32) -> f32 {
        Self::lerp(&self.0, x)
    }

    fn lerp(values: &[f32], x: f32) -> f32 {
        let last = values.len() - 1;
        let x = x.clamp(0.0, 1.0) * last as f32;
        let i = (x as usize).min(last.saturating_sub(1));
        let t = x - i as f32;
        let next = values[(i + 1).min(last)];
        values[i] + (next - values[i]) * t
    }
}

// MARK: Lookup tables
/// How a LUT profile encodes its profile connection space output.
#[derive(Clone, Copy)]
enum Pcs {
    Xyz,
    Lab,
    /// The ICC v2 16-bit Lab encoding, where 0xff00 is L* 100.
    LabV2,
}

/// A multidimensional grid of output colors.
struct Clut {
    grid: [usize; 4],
    strides: [usize; 4],
    inputs: usize,
    data: Vec<f32>,
}

impl Clut {
    fn parse(data: &[u8], grid: [usize; 4], inputs: usize, bytes: usize) -> Option<Self> {
        let mut strides = [0; 4];
        let mut points = 3usize;
        for i in (0..inputs).rev() {
            if grid[i] < 2 {
                return None;
            }
            strides[i] = points;
            points = points.checked_mul(grid[i])?;
        }
        let samples = data.get(..points.checked_mul(bytes)?)?;
        let data = if bytes == 1 {
            samples.iter().map(|v| f32::from(*v) / 255.0).collect()
        } else {
            samples
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| f32::from(u16::from_be_bytes(*v)) / 65535.0)
                .collect()
        };
        Some(Self {
            grid,
            strides,
            inputs,
            data,
        })
    }

    fn eval(&self, input: &[f32]) -> [f32; 3] {
        let mut base = 0;
        let mut fraction = [0.0f32; 4];
        for d in 0..self.inputs {
            let x = input[d].clamp(0.0, 1.0) * (self.grid[d] - 1) as f32;
            let i = (x as usize).min(self.grid[d] - 2);
            fraction[d] = x - i as f32;
            base += i * self.strides[d];
        }
        // A fourth input interpolates linearly between two tetrahedral lookups of the last
        // three.
        if self.inputs == 4 {
            let low = self.simplex(base, &fraction, 1);
            let high = self.simplex(base + self.strides[0], &fraction, 1);
            return core::array::from_fn(|c| low[c] + (high[c] - low[c]) * fraction[0]);
        }
        self.simplex(base, &fraction, 0)
    }

    // Walks from the base corner along the axes with the largest fractions first, which
    // interpolates within one simplex (tetrahedral interpolation for three inputs).
    fn simplex(&self, base: usize, fraction: &[f32; 4], first: usize) -> [f32; 3] {
        let mut order = [0, 1, 2, 3];
        let order = &mut order[first..self.inputs];
        order.sort_unstable_by(|a, b| fraction[*b].total_cmp(&fraction[*a]));
        let sample = |i: usize| [self.data[i], self.data[i + 1], self.data[i + 2]];
        let mut index = base;
        let mut previous = sample(index);
        let mut out = previous;
        for &d in order.iter() {
            index += self.strides[d];
            let current = sample(index);
            for c in 0..3 {
                out[c] += (current[c] - previous[c]) * fraction[d];
            }
            previous = current;
        }
        out
    }
}

/// An ICC `mft1`, `mft2` or `mAB ` pipeline from device samples to the PCS.
pub(crate) struct Lut {
    a_curves: Vec<Table>,
    clut: Option<Clut>,
    m_curves: Vec<Table>,
    matrix: Option<[[f32; 4]; 3]>,
    b_curves: Vec<Table>,
    pcs: Pcs,
    to_srgb: [[f32; 3]; 3],
}

impl Lut {
    fn parse(data: &[u8], inputs: usize, lab: bool) -> Option<Self> {
        let (&in_count, &out_count) = (data.get(8)?, data.get(9)?);
        if usize::from(in_count) != inputs || out_count != 3 {
            return None;
        }
        let pcs = |legacy| match (lab, legacy) {
            (false, _) => Pcs::Xyz,
            (true, false) => Pcs::Lab,
            (true, true) => Pcs::LabV2,
        };
        let to_srgb = xyz_d50_to_srgb().map(|row| row.map(|v| v as f32));
        match data.get(..4)? {
            kind @ (b"mft1" | b"mft2") => {
                let grid = usize::from(*data.get(10)?);
                let wide = kind == b"mft2";
                let (bytes, in_entries, out_entries, mut pos) = if wide {
                    let n = usize::from(be16(data, 48)?);
                    let m = usize::from(be16(data, 50)?);
                    (2, n, m, 52)
                } else {
                    (1, 256, 256, 48)
                };
                if !(2..=4096).contains(&in_entries) || !(2..=4096).contains(&out_entries) {
                    return None;
                }
                let tables = |count: usize, entries: usize, pos: &mut usize| {
                    (0..count)
                        .map(|_| {
                            let table = table(data.get(*pos..)?, entries, bytes)?;
                            *pos += entries * bytes;
                            Some(table)
                        })
                        .collect::<Option<Vec<_>>>()
                };
                let a_curves = tables(inputs, in_entries, &mut pos)?;
                let clut = Clut::parse(data.get(pos..)?, [grid; 4], inputs, bytes)?;
                pos += clut.data.len() * bytes;
                let b_curves = tables(3, out_entries, &mut pos)?;
                Some(Self {
                    a_curves,
                    clut: Some(clut),
                    m_curves: Vec::new(),
                    matrix: None,
                    b_curves,
                    pcs: pcs(wide),
                    to_srgb,
                })
            }
            b"mAB " => {
                let offset = |at: usize| be32(data, at).map(|v| v as usize);
                let curves = |at: usize, count: usize| -> Option<Vec<Table>> {
                    let mut pos = at;
                    (0..count)
                        .map(|_| {
                            let (curve, len) = Curve::parse(data.get(pos..)?)?;
                            pos = pos.checked_add(len)?;
                            Some(curve.table())
                        })
                        .collect()
                };
                let (b, matrix, m, clut, a) = (
                    offset(12)?,
                    offset(16)?,
                    offset(20)?,
                    offset(24)?,
                    offset(28)?,
                );
                if b == 0 || (clut != 0) != (a != 0) || (matrix != 0) != (m != 0) {
                    return None;
                }
                if clut == 0 && inputs != 3 {
                    return None;
                }
                let clut = if clut == 0 {
                    None
                } else {
                    let grid =
                        core::array::from_fn(|i| data.get(clut + i).map_or(0, |v| usize::from(*v)));
                    let bytes = usize::from(*data.get(clut + 16)?);
                    if !matches!(bytes, 1 | 2) {
                        return None;
                    }
                    Some(Clut::parse(data.get(clut + 20..)?, grid, inputs, bytes)?)
                };
                let matrix = if matrix == 0 {
                    None
                } else {
                    let value = |i: usize| s15_16(data, matrix + i * 4).map(|v| v as f32);
                    // Nine row-major coefficients, then the three offsets.
                    let mut rows = [[0.0; 4]; 3];
                    for (r, row) in rows.iter_mut().enumerate() {
                        for (c, cell) in row.iter_mut().enumerate() {
                            *cell = value(if c < 3 { r * 3 + c } else { 9 + r })?;
                        }
                    }
                    Some(rows)
                };
                Some(Self {
                    a_curves: if a == 0 {
                        Vec::new()
                    } else {
                        curves(a, inputs)?
                    },
                    clut,
                    m_curves: if m == 0 { Vec::new() } else { curves(m, 3)? },
                    matrix,
                    b_curves: curves(b, 3)?,
                    pcs: pcs(false),
                    to_srgb,
                })
            }
            _ => None,
        }
    }

    fn eval(&self, input: &[f32]) -> [f32; 3] {
        let mut v = [0.0; 4];
        for (i, value) in v.iter_mut().take(input.len()).enumerate() {
            *value = self.a_curves.get(i).map_or(input[i], |c| c.eval(input[i]));
        }
        self.eval_after_input_curves(v)
    }

    fn eval_after_input_curves(&self, v: [f32; 4]) -> [f32; 3] {
        let mut pcs = match &self.clut {
            Some(clut) => clut.eval(&v),
            None => [v[0], v[1], v[2]],
        };
        for (value, curve) in pcs.iter_mut().zip(&self.m_curves) {
            *value = curve.eval(*value);
        }
        if let Some(m) = &self.matrix {
            pcs = core::array::from_fn(|r| {
                m[r][0] * pcs[0] + m[r][1] * pcs[1] + m[r][2] * pcs[2] + m[r][3]
            });
        }
        for (value, curve) in pcs.iter_mut().zip(&self.b_curves) {
            *value = curve.eval(*value);
        }
        let xyz = match self.pcs {
            Pcs::Xyz => pcs.map(|v| v * (65535.0 / 32768.0)),
            Pcs::Lab | Pcs::LabV2 => {
                let scale = if matches!(self.pcs, Pcs::LabV2) {
                    65535.0 / 65280.0
                } else {
                    1.0
                };
                let [l, a, b] = pcs.map(|v| v * scale);
                lab_to_xyz([l * 100.0, a * 255.0 - 128.0, b * 255.0 - 128.0])
            }
        };
        core::array::from_fn(|r| {
            self.to_srgb[r][0] * xyz[0] + self.to_srgb[r][1] * xyz[1] + self.to_srgb[r][2] * xyz[2]
        })
    }
}

fn table(data: &[u8], entries: usize, bytes: usize) -> Option<Table> {
    let samples = data.get(..entries.checked_mul(bytes)?)?;
    Some(Table(if bytes == 1 {
        samples.iter().map(|v| f32::from(*v) / 255.0).collect()
    } else {
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| f32::from(u16::from_be_bytes(*v)) / 65535.0)
            .collect()
    }))
}

fn lab_to_xyz([l, a, b]: [f32; 3]) -> [f32; 3] {
    const E: f32 = 6.0 / 29.0;
    let f = |t: f32| {
        if t > E {
            t * t * t
        } else {
            3.0 * E * E * (t - 4.0 / 29.0)
        }
    };
    let y = (l + 16.0) / 116.0;
    [
        D50_XYZ[0] as f32 * f(y + a / 500.0),
        f(y),
        D50_XYZ[2] as f32 * f(y - b / 200.0),
    ]
}

/// Looks up ITU-T H.273 color primaries and transfer characteristics, or returns `None` for
/// unsupported code points.
pub(crate) fn cicp(primaries: u8, transfer: u8) -> Option<(Primaries, Curve)> {
    let primaries = match primaries {
        1 => SRGB,
        4 => BT470M,
        5 => BT470BG,
        6 | 7 => SMPTE170M,
        9 => BT2020,
        11 => DCI_P3,
        12 => DISPLAY_P3,
        22 => EBU3213,
        _ => return None,
    };
    let curve = match transfer {
        1 | 6 | 14 | 15 => Curve::Parametric([
            1.0 / 0.45,
            1.0 / 1.099,
            0.099 / 1.099,
            1.0 / 4.5,
            0.081,
            0.0,
            0.0,
        ]),
        4 => Curve::gamma(2.2),
        5 => Curve::gamma(2.8),
        8 => Curve::LINEAR,
        13 => Curve::SRGB,
        16 => Curve::Pq,
        18 => Curve::Hlg,
        _ => return None,
    };
    Some((primaries, curve))
}

// MARK: Transform
/// Converts samples of one color description to sRGB, clipping out-of-gamut colors.
pub(crate) enum Transform {
    /// Per-channel curves to linear light at each 8-bit sample, then a matrix to linear sRGB.
    Matrix {
        linear: Box<[[f32; 256]; 3]>,
        matrix: [[f32; 3]; 3],
    },
    Lut(Box<Lut>),
}

impl Transform {
    /// Builds a transform from RGB primaries and per-channel transfer curves, or returns
    /// `None` when the result is sRGB already.
    pub(crate) fn rgb(primaries: &Primaries, curves: [&Curve; 3]) -> Option<Self> {
        let matrix = multiply(&xyz_d50_to_srgb(), &rgb_to_xyz_d50(primaries)?);
        Self::matrix(matrix, curves)
    }

    /// Builds a transform from an ICC profile, or returns `None` for sRGB, unsupported or
    /// invalid profiles and profiles of another color model.
    pub(crate) fn icc(data: &[u8], model: ColorModel) -> Option<Self> {
        let size = be32(data, 0)? as usize;
        let data = data.get(..size)?;
        if size < 132 || data.get(36..40)? != b"acsp" {
            return None;
        }
        let space: &[u8] = match model {
            ColorModel::Gray => b"GRAY",
            ColorModel::Rgb => b"RGB ",
            ColorModel::Cmyk => b"CMYK",
        };
        if data.get(16..20)? != space
            || !matches!(data.get(12..16)?, b"mntr" | b"scnr" | b"prtr" | b"spac")
        {
            return None;
        }
        let lab = match data.get(20..24)? {
            b"XYZ " => false,
            b"Lab " => true,
            _ => return None,
        };
        let count = be32(data, 128)? as usize;
        let tag = |signature: &[u8]| {
            (0..count.min(data.len() / 12)).find_map(|i| {
                let entry = 132 + i * 12;
                if data.get(entry..entry + 4)? != signature {
                    return None;
                }
                let offset = be32(data, entry + 4)? as usize;
                data.get(offset..offset.checked_add(be32(data, entry + 8)? as usize)?)
            })
        };
        let curve = |signature: &[u8]| Some(Curve::parse(tag(signature)?)?.0);
        // Matrix/TRC profiles take precedence; LUTs cover the rest.
        if !lab {
            match model {
                ColorModel::Rgb => {
                    let column = |signature: &[u8]| {
                        let data = tag(signature).filter(|data| data.starts_with(b"XYZ "))?;
                        Some([s15_16(data, 8)?, s15_16(data, 12)?, s15_16(data, 16)?])
                    };
                    if let (Some(r), Some(g), Some(b), Some(rc), Some(gc), Some(bc)) = (
                        column(b"rXYZ"),
                        column(b"gXYZ"),
                        column(b"bXYZ"),
                        curve(b"rTRC"),
                        curve(b"gTRC"),
                        curve(b"bTRC"),
                    ) {
                        let to_xyz = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
                        invert(&to_xyz)?;
                        let matrix = multiply(&xyz_d50_to_srgb(), &to_xyz);
                        return Self::matrix(matrix, [&rc, &gc, &bc]);
                    }
                }
                ColorModel::Gray => {
                    if let Some(gray) = curve(b"kTRC") {
                        return Self::matrix(diagonal([1.0; 3]), [&gray, &gray, &gray]);
                    }
                }
                ColorModel::Cmyk => {}
            }
        }
        let lut = [b"A2B0", b"A2B1", b"A2B2"]
            .iter()
            .find_map(|signature| Lut::parse(tag(*signature)?, model.channels(), lab))?;
        Some(Self::Lut(Box::new(lut)))
    }

    fn matrix(matrix: Matrix, curves: [&Curve; 3]) -> Option<Self> {
        if matrix.iter().flatten().any(|v| !v.is_finite()) {
            return None;
        }
        let mut linear = Box::new([[0.0; 256]; 3]);
        for c in 0..3 {
            // RGB and gray profiles commonly share one transfer curve across channels.
            linear[c] = if let Some(previous) = (0..c).find(|&i| curves[i] == curves[c]) {
                linear[previous]
            } else {
                core::array::from_fn(|v| curves[c].eval(v as f64 / 255.0) as f32)
            };
        }
        let transform = Self::Matrix {
            linear,
            matrix: matrix.map(|row| row.map(|v| v as f32)),
        };
        (!transform.is_identity()).then_some(transform)
    }

    // Checks a lattice of colors and every gray, which catches sRGB profiles written with
    // slightly different but equivalent values.
    fn is_identity(&self) -> bool {
        let lattice = (0..16u8).flat_map(|r| {
            (0..16u8).flat_map(move |g| (0..16u8).map(move |b| [r * 17, g * 17, b * 17]))
        });
        let grays = (0..=255u8).map(|v| [v; 3]);
        lattice.chain(grays).all(|rgb| self.rgb8(rgb) == rgb)
    }

    // Evaluates normalized samples; 8-bit samples hit the matrix tables exactly.
    fn eval(&self, input: &[f32]) -> [u8; 3] {
        let linear = match self {
            Self::Matrix { linear, matrix } => {
                let v: [f32; 3] = core::array::from_fn(|i| Table::lerp(&linear[i], input[i]));
                core::array::from_fn(|i| {
                    matrix[i][0] * v[0] + matrix[i][1] * v[1] + matrix[i][2] * v[2]
                })
            }
            Self::Lut(lut) => lut.eval(&input[..lut.inputs()]),
        };
        linear.map(encode)
    }

    fn rgb8(&self, rgb: [u8; 3]) -> [u8; 3] {
        self.eval(&rgb.map(|v| f32::from(v) / 255.0))
    }

    /// Converts straight-alpha RGBA8 pixels in place; gray samples are read from red.
    pub(crate) fn apply(&self, pixels: &mut [u8]) {
        match self {
            // The linear light of every 8-bit sample is looked up once instead of per pixel,
            // which gives the same results as `rgb8`.
            Self::Matrix { linear, matrix } => {
                let linear: [[f32; 256]; 3] = core::array::from_fn(|i| {
                    core::array::from_fn(|v| Table::lerp(&linear[i], v as f32 / 255.0))
                });
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    let v: [f32; 3] = core::array::from_fn(|i| linear[i][usize::from(pixel[i])]);
                    for (i, row) in matrix.iter().enumerate() {
                        pixel[i] = encode(row[0] * v[0] + row[1] * v[1] + row[2] * v[2]);
                    }
                }
            }
            Self::Lut(lut) => {
                // Evaluate the input curves once per 8-bit value instead of per pixel.
                let inputs: [[f32; 256]; 3] = core::array::from_fn(|i| {
                    core::array::from_fn(|v| {
                        let value = v as f32 / 255.0;
                        lut.a_curves.get(i).map_or(value, |curve| curve.eval(value))
                    })
                });
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    let v = [
                        inputs[0][usize::from(pixel[0])],
                        inputs[1][usize::from(pixel[1])],
                        inputs[2][usize::from(pixel[2])],
                        0.0,
                    ];
                    let [r, g, b] = lut.eval_after_input_curves(v).map(encode);
                    pixel[..3].copy_from_slice(&[r, g, b]);
                }
            }
        }
    }

    /// Converts 16-bit samples to sRGB8; gray samples are read from red.
    pub(crate) fn rgb16(&self, rgb: [u16; 3]) -> [u8; 3] {
        self.eval(&rgb.map(|v| f32::from(v) / 65535.0))
    }

    /// Converts CMYK ink amounts, where 255 is full ink, to sRGB8.
    pub(crate) fn cmyk(&self, cmyk: [u8; 4]) -> [u8; 3] {
        self.eval(&cmyk.map(|v| f32::from(v) / 255.0))
    }
}

// Encodes linear light to the nearest 8-bit sRGB sample.
fn encode(linear: f32) -> u8 {
    (Table::lerp(&SRGB_ENCODE, linear) * 255.0 + 0.5) as u8
}

impl Lut {
    fn inputs(&self) -> usize {
        self.clut.as_ref().map_or(3, |clut| clut.inputs)
    }
}

// MARK: Reading
fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn s15_16(data: &[u8], at: usize) -> Option<f64> {
    Some(f64::from(be32(data, at)? as i32) / 65536.0)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_matches_std() {
        for i in 1..2000 {
            let x = f64::from(i) / 97.0;
            assert!(
                (exp(x / 4.0 - 10.0) - (x / 4.0 - 10.0).exp()).abs() < 1e-13,
                "exp {x}"
            );
            assert!((pow(x / 2000.0, 2.4) - (x / 2000.0).powf(2.4)).abs() < 1e-13);
        }
    }

    #[test]
    fn srgb_samples_round_trip_through_linear_light() {
        for value in 0..=255u8 {
            let encoded = f64::from(value) / 255.0;
            let expected = if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            };
            assert!((f64::from(srgb_to_linear(value)) - expected).abs() < 1e-6);
            assert_eq!(linear_to_srgb(srgb_to_linear(value)), value);
        }
        for i in 0..=10_000 {
            let linear = i as f32 / 10_000.0;
            let expected = (srgb_encode(f64::from(linear)) * 255.0).round() as u8;
            assert_eq!(linear_to_srgb(linear), expected, "{linear}");
        }
        assert_eq!(linear_to_srgb(-1.0), 0);
        assert_eq!(linear_to_srgb(2.0), 255);
    }

    #[test]
    fn srgb_descriptions_need_no_transform() {
        assert!(Transform::rgb(&SRGB, [&Curve::SRGB; 3]).is_none());
        let (primaries, curve) = cicp(1, 13).expect("sRGB code points");
        assert!(Transform::rgb(&primaries, [&curve; 3]).is_none());
        assert!(Transform::rgb(&DISPLAY_P3, [&Curve::SRGB; 3]).is_some());
        assert!(cicp(2, 13).is_none(), "unspecified primaries");
    }

    #[test]
    fn display_p3_red_clips_and_gray_stays_gray() {
        let transform = Transform::rgb(&DISPLAY_P3, [&Curve::SRGB; 3]).expect("P3 transform");
        let mut pixels = [255, 0, 0, 255, 128, 128, 128, 7];
        transform.apply(&mut pixels);
        assert_eq!(pixels, [255, 0, 0, 255, 128, 128, 128, 7]);
        let mut pixels = [200, 60, 40, 255];
        transform.apply(&mut pixels);
        assert!(pixels[0] > 200 && pixels[1] < 60, "{pixels:?}");
    }

    #[test]
    fn linear_light_is_encoded() {
        let transform = Transform::rgb(&SRGB, [&Curve::LINEAR; 3]).expect("linear transform");
        let mut pixels = [0, 1, 128, 255];
        transform.apply(&mut pixels);
        assert_eq!(pixels, [0, 13, 188, 255]);
    }

    #[test]
    fn shared_and_distinct_channel_curves_preserve_samples() {
        for gammas in [[2.2, 2.2, 2.2], [1.8, 2.2, 1.8], [1.8, 2.2, 2.4]] {
            let curves = gammas.map(Curve::gamma);
            let transform =
                Transform::matrix(diagonal([1.0; 3]), curves.each_ref()).expect("non-sRGB curves");
            let mut pixels: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v, v]).collect();
            transform.apply(&mut pixels);
            for (value, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let expected =
                    gammas.map(|gamma| linear_to_srgb((value as f64 / 255.0).powf(gamma) as f32));
                assert_eq!(pixel[..3], expected, "sample {value}, gamma {gammas:?}");
                assert_eq!(pixel[3], value as u8);
            }
        }
    }

    #[test]
    fn lut_batch_matches_individual_samples() {
        for inputs in [1, 3] {
            let data: Vec<u8> = (0..3 * 2usize.pow(inputs))
                .map(|i| (i * 37) as u8)
                .collect();
            let transform = Transform::Lut(Box::new(Lut {
                a_curves: (0..inputs)
                    .map(|i| Table(alloc::vec![0.0, 0.2 + i as f32 * 0.1, 1.0]))
                    .collect(),
                clut: Clut::parse(&data, [2; 4], inputs as usize, 1),
                m_curves: Vec::new(),
                matrix: None,
                b_curves: Vec::new(),
                pcs: Pcs::Xyz,
                to_srgb: xyz_d50_to_srgb().map(|row| row.map(|v| v as f32)),
            }));
            let original: Vec<u8> = (0..4096)
                .flat_map(|i| {
                    [
                        (i % 16 * 17) as u8,
                        (i / 16 % 16 * 17) as u8,
                        (i / 256 * 17) as u8,
                        i as u8,
                    ]
                })
                .collect();
            let mut pixels = original.clone();
            transform.apply(&mut pixels);
            for (actual, input) in pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(original.as_chunks::<4>().0)
            {
                assert_eq!(actual[..3], transform.rgb8([input[0], input[1], input[2]]));
                assert_eq!(actual[3], input[3]);
            }
        }
    }

    #[test]
    fn invalid_profiles_are_ignored() {
        assert!(Transform::icc(&[], ColorModel::Rgb).is_none());
        let mut data = alloc::vec![0; 132];
        data[..4].copy_from_slice(&132u32.to_be_bytes());
        data[12..24].copy_from_slice(b"mntrRGB XYZ ");
        data[36..40].copy_from_slice(b"acsp");
        assert!(Transform::icc(&data, ColorModel::Rgb).is_none());
        data[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Transform::icc(&data, ColorModel::Rgb).is_none());
        assert!(Transform::icc(&data, ColorModel::Gray).is_none());
        assert!(Transform::rgb(&[[0.3, 0.0]; 4], [&Curve::SRGB; 3]).is_none());
    }

    #[test]
    fn simplex_interpolation_is_exact_on_linear_data() {
        // A 2x2x2 grid holding the identity reproduces linear input everywhere.
        let mut data = Vec::new();
        for r in 0..2u8 {
            for g in 0..2u8 {
                for b in 0..2u8 {
                    data.extend([r, g, b].map(|v| v * 255));
                }
            }
        }
        let clut = Clut::parse(&data, [2; 4], 3, 1).expect("grid");
        let out = clut.eval(&[0.25, 0.5, 0.75, 0.0]);
        for (actual, expected) in out.iter().zip([0.25, 0.5, 0.75]) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn curve_elements_parse() {
        let mut curv = b"curv\0\0\0\0\0\0\0\x01\x01\xcd".to_vec();
        let (curve, len) = Curve::parse(&curv).expect("gamma curve");
        assert_eq!(len, 16);
        assert!((curve.eval(0.5) - 0.5f64.powf(461.0 / 256.0)).abs() < 1e-12);
        curv[11] = 3;
        curv.extend_from_slice(&[0xff, 0xff, 0, 0]);
        assert!(matches!(Curve::parse(&curv), Some((Curve::Table(_), 20))));
        let mut para = b"para\0\0\0\0\0\x05\0\0".to_vec();
        assert!(Curve::parse(&para).is_none());
        para[9] = 0;
        para.extend_from_slice(&0x0002_6666u32.to_be_bytes());
        let (curve, len) = Curve::parse(&para).expect("parametric curve");
        assert_eq!(len, 16);
        assert!((curve.eval(0.5) - 0.5f64.powf(2.4)).abs() < 1e-4);
    }
}
