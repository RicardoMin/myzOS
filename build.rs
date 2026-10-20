use std::{env, path::PathBuf};

fn main() {
    let script = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("kernel.ld");
    
    println!("cargo::rerun-if-changed={}", script.display());
    println!("cargo::rustc-link-arg=-T{}", script.display());

    println!("cargo::rerun-if-env-changed=MYZOS_USER_IMAGE");

    if env::var_os("CARGO_FEATURE_USER_VM_TEST").is_some() {
        let image = PathBuf::from(
            env::var_os("MYZOS_USER_IMAGE")
                .expect("set MYZOS_USER_IMAGE to the built init.bin"),
        )
        .canonicalize()
        .expect("user image path is invalid");
        
        println!("cargo::rerun-if-changed={}", image.display());
        println!("cargo::rustc-env=MYZOS_USER_IMAGE={}", image.display());
    }
}