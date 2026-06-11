use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::ptr;
use clap::Parser;
use std::fs::File;
use std::io::{self, Read};

include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

use libc::{
    _IOW, _IO, _IOR
};

const KVMIO : u32 = 0xae;
const KVM_CREATE_VM : u64 = _IO(KVMIO, 0x01);
const KVM_CREATE_VCPU : u64 = _IO(KVMIO, 0x41);
const KVM_GET_VCPU_MMAP_SIZE : u64 = _IO(KVMIO, 0x04);
const KVM_SET_USER_MEMORY_REGION : u64 = _IOW::<kvm_userspace_memory_region>(KVMIO, 0x46);
const KVM_GET_SREGS2 : u64 = _IOR::<kvm_sregs2>(KVMIO, 0xcc);
const KVM_SET_SREGS2 : u64 = _IOW::<kvm_sregs2>(KVMIO, 0xcd);
const KVM_GET_REGS : u64 = _IOR::<kvm_regs>(KVMIO, 0x81);
const KVM_SET_REGS : u64 = _IOW::<kvm_regs>(KVMIO, 0x82);
const KVM_RUN : u64 = _IO(KVMIO, 0x80);

pub struct KvmDev {
    pub file: File
}

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Binary to run
    binary: String,

    /// Memory size in kilobytes
    #[arg(short, long, default_value_t = 256)]
    memory: usize,
}

fn load_binary(path: &str) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut res = Vec::new();
    file.read_to_end(&mut res)?;

    Ok(res)
}

impl KvmDev {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
	let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .expect("Failed to open /dev/kvm");
	Ok(Self { file })
    }

    fn fd(&self) -> libc::c_int {
	self.file.as_raw_fd()
    }
}

pub struct MemRegion {
    pub mem_ptr: *mut libc::c_void,
    pub mem_size: usize,
    pub guest_phys_addr: u64
}

impl MemRegion {
    fn new(vm_fd: libc::c_int, mem_size: usize, guest_phys_addr: u64) -> Result<Self, Box<dyn std::error::Error>> {
	// 140900 mmap(NULL, 1075838976, 0 /* PROT_NONE */, 0x22 /* MAP_PRIVATE|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
	// 140900 mmap(0x7768b3e00000, 1073741824, 0x3 /* PROT_READ|PROT_WRITE */, 0x32 /* MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
	let mem_ptr = unsafe {
	    libc::mmap(ptr::null_mut(),
		       mem_size as usize,
		       libc::PROT_READ|libc::PROT_WRITE,
		       libc::MAP_PRIVATE|libc::MAP_ANONYMOUS,
		       -1,
		       0)
	};
	if mem_ptr == libc::MAP_FAILED {
            return Err(Box::new(io::Error::last_os_error()));
	}

	// 140900 ioctl(9<anon_inode:kvm-vm>, 0x4020ae46 /* KVM_SET_USER_MEMORY_REGION */, {slot=0, flags=0, guest_phys_addr=0, memory_size=1073741824, userspace_addr=0x7768b3e00000}) = 0
	let region = kvm_userspace_memory_region {
	    slot : 0,
	    flags : 0,
	    guest_phys_addr : guest_phys_addr,
	    memory_size : mem_size as u64,
	    userspace_addr : mem_ptr as u64
	};

	let ret = unsafe { libc::ioctl(vm_fd, KVM_SET_USER_MEMORY_REGION, &region) };
	if ret < 0 {
            return Err(Box::new(io::Error::last_os_error()));
	}

	Ok(Self { mem_ptr, mem_size, guest_phys_addr })
    }
}

impl Drop for MemRegion {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.mem_ptr, self.mem_size); }
    }
}

fn create_vm(kvm_dev: &KvmDev) -> Result<libc::c_int, Box<dyn std::error::Error>> {

    let vm_fd = unsafe { libc::ioctl(kvm_dev.fd(), KVM_CREATE_VM, 0usize) };
    if vm_fd < 0 {
        return Err(Box::new(io::Error::last_os_error()));
    }

    Ok(vm_fd)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let kvm_dev = KvmDev::new()?;
    let vm_fd = create_vm(&kvm_dev)?;

    let mem_region = MemRegion::new(vm_fd, args.memory * 1024, 0x0)?;

    // 140904 ioctl(9<anon_inode:kvm-vm>, 0xae41 /* KVM_CREATE_VCPU */, 0) = 10<anon_inode:kvm-vcpu:0>
    let vcpu_fd = unsafe { libc::ioctl(vm_fd, KVM_CREATE_VCPU, 0usize) };

    // 140904 ioctl(3</dev/kvm<char 10:232>>, 0xae04 /* KVM_GET_VCPU_MMAP_SIZE */, 0) = 12288
    // 140904 mmap(NULL, 12288, 0x3 /* PROT_READ|PROT_WRITE */, 0x1 /* MAP_SHARED */, 10<anon_inode:kvm-vcpu:0>, 0) = 0x7769050b0000
    let kvm_run_size = unsafe { libc::ioctl(kvm_dev.fd(), KVM_GET_VCPU_MMAP_SIZE, 0usize) };
    let kvm_run_mem = unsafe { libc::mmap(ptr::null_mut(), kvm_run_size as usize, libc::PROT_READ|libc::PROT_WRITE, libc::MAP_SHARED, vcpu_fd, 0) };

    // 140904 ioctl(10<anon_inode:kvm-vcpu:0>, 0x8140aecc /* KVM_GET_SREGS2 */, 0x77690198f310) = 0
    let mut sregs : kvm_sregs2;
    unsafe {
	sregs = std::mem::zeroed();
	libc::ioctl(vcpu_fd, KVM_GET_SREGS2, &mut sregs);
    }

    sregs.cs.base = 0;
    sregs.cs.selector = 0;

    println!("CS\tbase={:#x}\n\tselector={:#x}\nCR0={:#x}", sregs.cs.base, sregs.cs.selector, sregs.cr0);

    unsafe {
	libc::ioctl(vcpu_fd, KVM_SET_SREGS2, &mut sregs);
    }

    // 140904 ioctl(10<anon_inode:kvm-vcpu:0>, 0x8090ae81 /* KVM_GET_REGS */, {rax=0, ..., rsp=0, rbp=0, ..., rip=0xfff0, rflags=0x2}) = 0
    let mut regs : kvm_regs;
    unsafe {
	regs = std::mem::zeroed();
	libc::ioctl(vcpu_fd, KVM_GET_REGS, &mut regs);
    }

    regs.rip = 0x1000;

    println!("RIP={:#x} RSP={:#x} RFLAGS={:#x}", regs.rip, regs.rsp, regs.rflags);

    unsafe {
	libc::ioctl(vcpu_fd, KVM_SET_REGS, &mut regs)
    };

    let code = load_binary(args.binary.as_str())?;

    unsafe {
	std::ptr::copy_nonoverlapping(
	    code.as_ptr(),
	    (mem_region.mem_ptr as *mut u8).add(0x1000),
	    code.len(),
	)
    };

    unsafe {
	libc::ioctl(vcpu_fd, KVM_RUN, 0usize)
    };

    let exit_reason = unsafe { (*(kvm_run_mem as *mut kvm_run)).exit_reason };

    println!("EXIT REASON = {}", exit_reason);

    Ok(())
}
