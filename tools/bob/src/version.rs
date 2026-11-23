/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::{self, Display, Formatter};

/// A numeric version with up to three components, as used by Android tooling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Version {
    pub(crate) major: u16,
    pub(crate) minor: u16,
    pub(crate) patch: u16,
}

impl Version {
    /// Parses `major`, `major.minor`, or `major.minor.patch`.
    pub(crate) fn parse(version: &str) -> Result<Self, String> {
        let mut parts = version.split('.');
        let major = parse_component(parts.next(), "major")?;
        let minor = parse_optional_component(parts.next(), "minor")?;
        let patch = parse_optional_component(parts.next(), "patch")?;
        if parts.next().is_some() {
            return Err("version has more than three components".to_string());
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl Display for Version {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let Self {
            major,
            minor,
            patch,
        } = self;
        write!(formatter, "{major}.{minor}.{patch}")
    }
}

fn parse_component(component: Option<&str>, name: &str) -> Result<u16, String> {
    component
        .ok_or_else(|| format!("missing {name} version"))?
        .parse()
        .map_err(|_| format!("invalid {name} version"))
}

fn parse_optional_component(component: Option<&str>, name: &str) -> Result<u16, String> {
    component.map_or(Ok(0), |component| {
        component
            .parse()
            .map_err(|_| format!("invalid {name} version"))
    })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_to_three_components() {
        assert_eq!(
            Version::parse("35").unwrap(),
            Version {
                major: 35,
                minor: 0,
                patch: 0,
            }
        );
        assert_eq!(
            Version::parse("35.1").unwrap(),
            Version {
                major: 35,
                minor: 1,
                patch: 0,
            }
        );
        assert_eq!(Version::parse("35.1.2").unwrap().to_string(), "35.1.2");
    }

    #[test]
    fn orders_versions_numerically() {
        assert!(Version::parse("35.10").unwrap() > Version::parse("35.2").unwrap());
    }

    #[test]
    fn rejects_invalid_versions() {
        for version in ["", "1.", "1..3", "1.2.3.4", "a.b.c", "65536"] {
            assert!(Version::parse(version).is_err(), "version: {version}");
        }
    }
}
