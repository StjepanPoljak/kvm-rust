use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::ptr;

include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

use libc::{
    PROT_NONE, PROT_READ, PROT_WRITE,
    MAP_SHARED, MAP_PRIVATE, MAP_ANONYMOUS, MAP_FIXED,
    _IOW, _IO, _IOR
};

const KVMIO : u32 = 0xae;
const KVM_CREATE_VM : u64 = _IO(KVMIO, 0x01);
const KVM_CREATE_VCPU : u64 = _IO(KVMIO, 0x41);
const KVM_GET_VCPU_MMAP_SIZE : u64 = _IO(KVMIO, 0x04);
const KVM_GET_SREGS : u64 = _IOR::<kvm_sregs2>(KVMIO, 0xcc);
const KVM_SET_USER_MEMORY_REGION : u64 = _IOW::<kvm_userspace_memory_region>(0xae, 0x46);


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .expect("failed to open /dev/kvm");

    let fd = file.as_raw_fd();
    let vm_fd = unsafe { libc::ioctl(fd, KVM_CREATE_VM, 0usize) };

    // 140904 ioctl(3</dev/kvm<char 10:232>>, 0xae04 /* KVM_GET_VCPU_MMAP_SIZE */, 0) = 12288
    // 140904 mmap(NULL, 12288, 0x3 /* PROT_READ|PROT_WRITE */, 0x1 /* MAP_SHARED */, 10<anon_inode:kvm-vcpu:0>, 0) = 0x7769050b0000
    let kvm_run_size = unsafe { libc::ioctl(fd, KVM_GET_VCPU_MMAP_SIZE, 0usize) };
    let kvm_run_mem = unsafe { libc::mmap(ptr::null_mut(), kvm_run_size as usize, PROT_READ|PROT_WRITE, MAP_SHARED, -1, 0) };

    // 140900 mmap(NULL, 1075838976, 0 /* PROT_NONE */, 0x22 /* MAP_PRIVATE|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
    // 140900 mmap(0x7768b3e00000, 1073741824, 0x3 /* PROT_READ|PROT_WRITE */, 0x32 /* MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
    let mem_size: u64 = 256 * 1024;
    let mem: *mut libc::c_void = unsafe { libc::mmap(ptr::null_mut(), (mem_size + (1024 * 1024 * 2)) as usize, PROT_NONE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0) };
    _ = unsafe { libc::mmap(ptr::null_mut(), mem_size as usize, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS, -1, 0) };

    // 140900 ioctl(9<anon_inode:kvm-vm>, 0x4020ae46 /* KVM_SET_USER_MEMORY_REGION */, {slot=0, flags=0, guest_phys_addr=0, memory_size=1073741824, userspace_addr=0x7768b3e00000}) = 0
    let region = kvm_userspace_memory_region {
	slot : 0,
	flags : 0,
	guest_phys_addr : 0,
	memory_size : mem_size,
	userspace_addr : mem as u64
    };

    /* TODO: Add check here later for ret value. */
    let _ret = unsafe { libc::ioctl(vm_fd, KVM_SET_USER_MEMORY_REGION, &region) };

    // 140904 ioctl(9<anon_inode:kvm-vm>, 0xae41 /* KVM_CREATE_VCPU */, 0) = 10<anon_inode:kvm-vcpu:0>
    let vcpu_fd = unsafe { libc::ioctl(vm_fd, KVM_CREATE_VCPU, 0usize) };

    unsafe {
        let mut sregs : kvm_sregs2 = std::mem::zeroed();
	// 140904 ioctl(10<anon_inode:kvm-vcpu:0>, 0x8140aecc /* KVM_GET_SREGS2 */, 0x77690198f310) = 0
	libc::ioctl(vcpu_fd, KVM_GET_SREGS, &mut sregs);
    }

    Ok(())
}
