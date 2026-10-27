//! Search verdicts and calibration boundary from SPEC §7.2 (internal crate name only; user-facing text says "inVectus search"). Implementation starts after P0.

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_is_available() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
    }
}
