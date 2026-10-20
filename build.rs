use std::{env, path::PathBuf};

fn main() {
    let script = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("kernel.ld");
    
    println!("cargo::rerun-if-changed={}", script.display());
    println!("cargo::rustc-link-arg=-T{}", script.display());
}