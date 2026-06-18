use bindgen;
use std::path::PathBuf;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = PathBuf::from(env::var("OUT_DIR")?);

    bindgen::Builder::default()
        .header("/usr/include/linux/kvm.h")
        .allowlist_type("kvm_sregs2")
        .allowlist_type("kvm_userspace_memory_region")
        .allowlist_type("kvm_regs")
        .allowlist_type("kvm_run")
        .generate_comments(false)
        .generate()?
        .write_to_file(out_path.join("kvm-bindings.rs"))?;

    Ok(())
}

