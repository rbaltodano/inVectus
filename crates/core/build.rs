//! Compiles `cpp/duckdb_os_random.cpp` against the bundled DuckDB sources that `libduckdb-sys`
//! unpacked (it publishes their include directory as `DEP_DUCKDB_INCLUDE`). The flags mirror
//! `libduckdb-sys`'s own bundled build so both sides agree on DuckDB's class layouts.

use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=cpp/duckdb_os_random.cpp");
    let include = PathBuf::from(
        env::var("DEP_DUCKDB_INCLUDE")
            .expect("libduckdb-sys (bundled) publishes DEP_DUCKDB_INCLUDE"),
    );
    // <out>/duckdb/src/include -> <out>/duckdb
    let root = include
        .parent()
        .and_then(|p| p.parent())
        .expect("DuckDB source root")
        .to_path_buf();

    let windows = env::var("CARGO_CFG_WINDOWS").is_ok();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|v| v == "msvc");
    let debug = env::var("DEBUG").is_ok_and(|v| v != "false" && v != "0");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("cpp/duckdb_os_random.cpp")
        .include(&include)
        .include(root.join("third_party/mbedtls/include"))
        .include(root.join("third_party/mbedtls/library"))
        .flag_if_supported("-std=c++17")
        .flag_if_supported("/std:c++17")
        .flag_if_supported("/utf-8")
        .warnings(false);
    if msvc {
        build.flag("/EHsc");
    }
    if debug {
        build.define("DEBUG", None);
    } else {
        build.define("NDEBUG", None);
    }
    if windows {
        build.define("DUCKDB_BUILD_LIBRARY", None);
    }
    build.compile("invectus_duckdb_os_random");
    if windows {
        println!("cargo:rustc-link-lib=bcrypt");
    }
}
