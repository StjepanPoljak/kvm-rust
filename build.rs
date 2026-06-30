use bindgen;
use std::path::PathBuf;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = PathBuf::from(env::var("OUT_DIR")?);
    let target = std::env::var("TARGET")?;

    match target.as_str() {
        "x86_64-unknown-linux-gnu" => {
            bindgen::Builder::default()
                .header("/usr/include/linux/kvm.h")
                .allowlist_type("kvm_sregs2")
                .allowlist_type("kvm_userspace_memory_region")
                .allowlist_type("kvm_regs")
                .allowlist_type("kvm_run")
                .generate_comments(false)
                .generate()?
                .write_to_file(out_path.join("kvm-bindings.rs"))?;

            bindgen::Builder::default()
                .header("/usr/include/x86_64-linux-gnu/asm/bootparam.h")
                .allowlist_type("boot_params")
                .blocklist_type("__u8")
                .blocklist_type("__u16")
                .blocklist_type("__u32")
                .blocklist_type("__u64")
                .generate_comments(false)
                .generate()?
                .write_to_file(out_path.join("bootparam-bindings.rs"))?; }

        "aarch64-unknown-linux-gnu" => {
            bindgen::Builder::default()
                .header("/usr/include/linux/kvm.h")
                .clang_arg(format!("--target={target}"))
                .clang_arg("-I/usr/aarch64-linux-gnu/include")
                .allowlist_type("kvm_userspace_memory_region")
                .allowlist_type("kvm_regs")
                .allowlist_type("kvm_run")
                .generate_comments(false)
                .generate()?
                .write_to_file(out_path.join("kvm-bindings.rs"))?;
        }
        _ => ()
    }

    Ok(())
}

