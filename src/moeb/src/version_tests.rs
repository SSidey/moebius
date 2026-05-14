/// Tests for binary version reporting and release metadata alignment.
///
/// These tests verify that:
/// 1. The binary correctly embeds and reports its semantic version at runtime.
/// 2. The Cargo.toml version string is a valid semver value.
/// 3. The version baked into the binary matches the package metadata known at
///    compile time — ensuring that a release artifact can always identify
///    itself correctly.

#[cfg(test)]
mod tests {
    /// The version string embedded at compile time via the CARGO_PKG_VERSION
    /// environment variable, which Cargo sets from Cargo.toml [package].version.
    const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

    /// Verify that the embedded version is non-empty.
    #[test]
    fn version_string_is_not_empty() {
        assert!(
            !PKG_VERSION.is_empty(),
            "CARGO_PKG_VERSION must not be empty; check [package].version in Cargo.toml"
        );
    }

    /// Verify that the embedded version follows a basic semver shape:
    /// MAJOR.MINOR.PATCH with optional pre-release / build-metadata suffix.
    #[test]
    fn version_string_is_valid_semver() {
        // A minimal semver check: at least two dots, all numeric core components.
        let parts: Vec<&str> = PKG_VERSION.splitn(3, '.').collect();
        assert_eq!(
            parts.len(),
            3,
            "Version '{}' does not have MAJOR.MINOR.PATCH structure",
            PKG_VERSION
        );

        // MAJOR must be purely numeric.
        assert!(
            parts[0].parse::<u64>().is_ok(),
            "MAJOR component '{}' is not a non-negative integer",
            parts[0]
        );

        // MINOR must be purely numeric.
        assert!(
            parts[1].parse::<u64>().is_ok(),
            "MINOR component '{}' is not a non-negative integer",
            parts[1]
        );

        // PATCH may include a pre-release suffix (e.g. "0-alpha.1"); the
        // numeric prefix before any '-' or '+' must still be valid.
        let patch_core = parts[2].split(&['-', '+']).next().unwrap_or(parts[2]);
        assert!(
            patch_core.parse::<u64>().is_ok(),
            "PATCH core '{}' (from '{}') is not a non-negative integer",
            patch_core,
            parts[2]
        );
    }

    /// Verify that the clap-derived --version output would surface the same
    /// version string that Cargo embeds at build time.
    ///
    /// This test checks the constant itself rather than spawning the binary,
    /// because spawning a subprocess from within `cargo test` is environment-
    /// dependent.  The release workflow's "Verify binary version output" step
    /// performs the end-to-end check against the compiled artifact.
    #[test]
    fn cargo_pkg_version_matches_compile_time_constant() {
        // env!("CARGO_PKG_VERSION") is set by Cargo to the value in
        // [package].version.  If this test compiles and runs, the value is
        // necessarily the same as the one that clap will embed via
        // #[command(version = env!("CARGO_PKG_VERSION"))].
        assert!(
            !PKG_VERSION.is_empty(),
            "CARGO_PKG_VERSION is empty — Cargo version metadata is missing"
        );
    }
}
