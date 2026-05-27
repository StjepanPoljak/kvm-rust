use bindgen;
use std::path::PathBuf;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());


    println!("cargo:rerun-if-changed=wrapper.h");

    bindgen::Builder::default()
	.header("/usr/include/linux/kvm.h")
	.clang_arg("-I/usr/include")
	.derive_default(true)
	.allowlist_type("kvm_sregs2")
	.allowlist_type("kvm_userspace_memory_region")
	.parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate_comments(false)
	.generate()
	.unwrap()
	.write_to_file(out_path.join("kvm-bindings.rs")).unwrap();

    Ok(())
}

