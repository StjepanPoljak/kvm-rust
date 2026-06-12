use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::ptr;
use std::fs::File;
use std::io::{self};

include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

use libc::{
    _IOW, _IO, _IOR
};

const KVMIO : u32 = 0xae;
const KVM_CREATE_VM : u64 = _IO(KVMIO, 0x01);
const KVM_SET_USER_MEMORY_REGION : u64 = _IOW::<kvm_userspace_memory_region>(KVMIO, 0x46);

pub struct KvmDev {
    pub file: File
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

    fn create_vm(&self) -> Result<VM, Box<dyn std::error::Error>> {
	// 140900 ioctl(3</dev/kvm<char 10:232>>, 0xae01 /* KVM_CREATE_VM */, 0) = 9<anon_inode:kvm-vm>
        let vm_fd = unsafe { libc::ioctl(self.fd(), KVM_CREATE_VM, 0usize) };
        if vm_fd < 0 {
            return Err(Box::new(io::Error::last_os_error()));
        }

        Ok(VM { fd: vm_fd })
    }

    fn fd(&self) -> libc::c_int {
        self.file.as_raw_fd()
    }
}

pub struct VM {
    pub fd: libc::c_int
}

pub struct MemRegion {
    pub mem_ptr: *mut libc::c_void,
    pub mem_size: usize,
    pub guest_phys_addr: u64
}

impl Drop for MemRegion {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.mem_ptr, self.mem_size); }
    }
}

impl VM {
    fn create_mem_region(&self, mem_size: usize, guest_phys_addr: u64) -> Result<MemRegion, Box<dyn std::error::Error>> {
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

        let ret = unsafe { libc::ioctl(self.fd, KVM_SET_USER_MEMORY_REGION, &region) };
        if ret < 0 {
            return Err(Box::new(io::Error::last_os_error()));
        }

        Ok(MemRegion { mem_ptr, mem_size, guest_phys_addr })
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let kvm_dev = KvmDev::new()?;
    let vm = kvm_dev.create_vm()?;
    let mem_region = vm.create_mem_region(256 * 1024, 0x0)?;

    Ok(())
}
