fn main() {
    println!("inVectus {} — P0 scaffold", invectus_core::VERSION);
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_is_available() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0");
    }
}
