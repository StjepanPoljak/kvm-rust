use bindgen;
use std::path::PathBuf;
use std::env;

fn virtio_bindings(out_path: &PathBuf, clang_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    bindgen::Builder::default()
        .clang_args(clang_args)
        .header("/usr/include/linux/virtio_blk.h")
        .header("/usr/include/linux/virtio_net.h")
        .header("/usr/include/linux/virtio_mmio.h")
        .allowlist_type("virtio_blk_config")
        .allowlist_type("virtio_blk_outhdr")
        .allowlist_type("virtio_net_hdr_v1")
        .allowlist_var("VIRTIO_ID_.*")
        .allowlist_var("VIRTIO_MMIO_.*")
        .allowlist_var("VIRTIO_BLK_.*")
        .allowlist_var("VIRTIO_F_.*")
        .allowlist_var("VIRTIO_CONFIG_S_.*")
        .generate_comments(false)
        .generate()?
        .write_to_file(out_path.join("virtio-blk-bindings.rs"))?;

    bindgen::Builder::default()
        .clang_args(clang_args)
        .header("/usr/include/linux/virtio_ring.h")
        .allowlist_type("vring_desc")
        .allowlist_type("vring_avail")
        .allowlist_type("vring_used")
        .allowlist_type("vring_used_elem")
        .allowlist_var("VRING_.*")
        .allowlist_var("VRING_AVAIL_F_NO_INTERRUPT")
        .blocklist_type("__u8")
        .blocklist_type("__u16")
        .blocklist_type("__u32")
        .blocklist_type("__u64")
        .blocklist_type("__virtio16")
        .blocklist_type("__virtio32")
        .blocklist_type("__virtio64")
        .generate_comments(false)
        .generate()?
        .write_to_file(out_path.join("virtio-ring-bindings.rs"))?;

    Ok(())
}

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
                .allowlist_type("kvm_cpuid2")
                .allowlist_type("kvm_cpuid_entry2")
                .allowlist_type("kvm_pit_config")
                .allowlist_type("kvm_irq_level")
                .allowlist_var("KVMIO")
                .allowlist_var("KVM_EXIT_.*")
                .allowlist_var("KVM_INTERNAL_ERROR_.*")
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
                .write_to_file(out_path.join("bootparam-bindings.rs"))?;

            virtio_bindings(&out_path, &[])?; }

        "aarch64-unknown-linux-gnu" => {
            bindgen::Builder::default()
                .header("/usr/include/linux/kvm.h")
                .clang_arg(format!("--target={target}"))
                .clang_arg("-I/usr/aarch64-linux-gnu/include")
                .allowlist_type("kvm_userspace_memory_region")
                .allowlist_type("kvm_one_reg")
                .allowlist_type("kvm_vcpu_init")
                .allowlist_type("kvm_run")
                .allowlist_type("kvm_create_device")
                .allowlist_type("kvm_device_attr")
                .allowlist_type("kvm_irq_level")
                .allowlist_type("kvm_device_type")
                .constified_enum_module("kvm_device_type")
                .allowlist_var("KVMIO")
                .allowlist_var("KVM_EXIT_.*")
                .allowlist_var("KVM_INTERNAL_ERROR_.*")
                .allowlist_var("KVM_REG_.*")
                .allowlist_var("KVM_VGIC_V2_.*")
                .allowlist_var("KVM_DEV_ARM_VGIC_.*")
                .allowlist_var("KVM_ARM_IRQ_TYPE_.*")
                .generate_comments(false)
                .generate()?
                .write_to_file(out_path.join("kvm-bindings.rs"))?;

            virtio_bindings(&out_path, &[format!("--target={target}"),
                                         "-I/usr/aarch64-linux-gnu/include".to_string()])?;
        }
        _ => ()
    }

    Ok(())
}

