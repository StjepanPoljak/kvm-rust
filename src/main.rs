#[allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code)]
mod kvm_bindings {
    include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));
}
use kvm_bindings::*;

include!("frontend/mod.rs");
include!("util/mod.rs");
include!("arch/mod.rs");

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use clap::Parser;
use std::fs::File;
use std::io::{self, Read};
use libc::{_IOW, _IO};
use std::collections::HashMap;
use std::sync::{Arc,Mutex};
use std::sync::atomic::{AtomicBool, Ordering, AtomicU64};

type MMIO = kvm_run__bindgen_ty_1__bindgen_ty_6;
type IO = kvm_run__bindgen_ty_1__bindgen_ty_4;
type MMIODevices = HashMap::<u64, Arc<Mutex<dyn MMIODevice>>>;
type IODevices = HashMap::<u64, Arc<Mutex<dyn IODevice>>>;
type GuestMemory = Vec<MemRegion>;

pub trait MMIODevice {
    fn handle(&mut self, mmio: &mut MMIO, mem: &GuestMemory) -> io::Result<()>;
}

pub trait IODevice {
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()>;
}

static SHOULD_STOP: AtomicBool = AtomicBool::new(false);
static MAIN_TID: AtomicU64 = AtomicU64::new(0);

const KVM_CREATE_VM: libc::Ioctl = _IO(KVMIO, 0x01);
const KVM_CREATE_VCPU: libc::Ioctl = _IO(KVMIO, 0x41);
const KVM_GET_VCPU_MMAP_SIZE: libc::Ioctl = _IO(KVMIO, 0x04);

const KVM_SET_USER_MEMORY_REGION: libc::Ioctl = _IOW::<kvm_userspace_memory_region>(KVMIO, 0x46);
const KVM_RUN: libc::Ioctl = _IO(KVMIO, 0x80);

#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    /// Binary to run
    #[arg(short, long)]
    binary: Option<String>,

    /// vmlinux or Image to run
    #[arg(long)]
    linux: Option<String>,

    /// Memory size in kilobytes
    #[arg(short, long, default_value_t = 256)]
    memory: usize,

    /// Load address
    #[arg(long, default_value_t = 0x1000)]
    load_addr: u64,

    /// Device tree blob
    #[arg(short, long)]
    dtb:Option<String>,

    /// Initramfs path
    #[arg(short, long)]
    initramfs:Option<String>
}

pub struct KvmDev {
    pub file: File,
    pub kvm_run_size: usize
}

impl KvmDev {
    fn new() -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .expect("Failed to open /dev/kvm");

        // 140904 ioctl(3</dev/kvm<char 10:232>>, 0xae04 /* KVM_GET_VCPU_MMAP_SIZE */, 0) = 12288
        let kvm_run_size = unsafe { libc::ioctl(file.as_raw_fd(), KVM_GET_VCPU_MMAP_SIZE, 0usize) };
        if kvm_run_size < 0 {
            return Err(io::Error::last_os_error());
        }
        
        Ok(Self { file, kvm_run_size: kvm_run_size as usize })
    }

    fn create_vm(&self) -> io::Result<VM> {
        // 140900 ioctl(3</dev/kvm<char 10:232>>, 0xae01 /* KVM_CREATE_VM */, 0) = 9<anon_inode:kvm-vm>
        let vm_fd = unsafe { libc::ioctl(self.fd(), KVM_CREATE_VM, 0usize) };
        if vm_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(VM {
            fd: vm_fd,
            guest_memory: GuestMemory::new(),
            mmio_devices: MMIODevices::new(),
            io_devices: IODevices::new() })
    }

    fn fd(&self) -> libc::c_int {
        self.file.as_raw_fd()
    }

    fn get_kvm_run_size(&self) -> usize {
        self.kvm_run_size
    }
}

pub struct VM {
    pub fd: libc::c_int,
    pub guest_memory: GuestMemory,
    pub mmio_devices: MMIODevices,
    pub io_devices: IODevices
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

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
fn gpa_to_host(mem: &GuestMemory, gpa: u64, len: usize) -> Option<*mut u8> {
    mem.iter().find_map(|r| {
        let off = usize::try_from(gpa.checked_sub(r.guest_phys_addr)?).ok()?;
        (off.checked_add(len)? <= r.mem_size).then(|| unsafe { (r.mem_ptr as *mut u8).add(off) })
    })
}

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
fn read_guest_ptr<T>(ptr: *const u8) -> T {
    unsafe {
        std::ptr::read_volatile(ptr.cast::<T>())
    }
}

impl VM {
    fn create_vcpu(&self) -> io::Result<VCPU> {
        // 140904 ioctl(9<anon_inode:kvm-vm>, 0xae41 /* KVM_CREATE_VCPU */, 0) = 10<anon_inode:kvm-vcpu:0>
        let vcpu_fd = unsafe { libc::ioctl(self.fd, KVM_CREATE_VCPU, 0usize) };
        if vcpu_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok( VCPU { fd: vcpu_fd, kvm_run_mem: std::ptr::null_mut() } )
    }

    fn add_mem_region(&mut self, mem_size: usize, guest_phys_addr: u64) -> io::Result<usize> {
        // 140900 mmap(NULL, 1075838976, 0 /* PROT_NONE */, 0x22 /* MAP_PRIVATE|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
        // 140900 mmap(0x7768b3e00000, 1073741824, 0x3 /* PROT_READ|PROT_WRITE */, 0x32 /* MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
        let mem_ptr = unsafe {
            libc::mmap(std::ptr::null_mut(),
                       mem_size as usize,
                       libc::PROT_READ|libc::PROT_WRITE,
                       libc::MAP_PRIVATE|libc::MAP_ANONYMOUS,
                       -1,
                       0)
        };
        if mem_ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        // 140900 ioctl(9<anon_inode:kvm-vm>, 0x4020ae46 /* KVM_SET_USER_MEMORY_REGION */, {slot=0, flags=0, guest_phys_addr=0, memory_size=1073741824, userspace_addr=0x7768b3e00000}) = 0
        let region = kvm_userspace_memory_region {
            slot : self.guest_memory.len() as u32,
            flags : 0,
            guest_phys_addr : guest_phys_addr,
            memory_size : mem_size as u64,
            userspace_addr : mem_ptr as u64
        };

        let ret = unsafe { libc::ioctl(self.fd, KVM_SET_USER_MEMORY_REGION, &region) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        self.guest_memory.push(MemRegion { mem_ptr, mem_size, guest_phys_addr });

        Ok(self.guest_memory.len() - 1)
    }

    fn load_data_to_memory(&self, mem_region_idx: usize, data: Vec<u8>, offset: u64) -> io::Result<usize> {
        let mem_region = self.guest_memory
            .get(mem_region_idx)
            .ok_or(io::Error::other("Index exceeds memory region vector."))?;

        if (offset as usize) >= mem_region.mem_size {
            return Err(io::Error::other("Offset exceeds memory region."));
        }

        if (offset as usize) + data.len() > mem_region.mem_size {
            return Err(io::Error::other(format!("Data ({:#x} {:#x}) would exceed memory region ({}).", offset, data.len(), mem_region.mem_size)));
        }

        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(),
                                          (mem_region.mem_ptr as *mut u8).add(offset as usize),
                                          data.len());
        };

        Ok(data.len())
    }

    fn load_file_to_memory(&self, mem_region_idx: usize, path: &str, offset: u64) -> io::Result<usize> {
        let mut file = File::open(path)?;
        let mut res = Vec::new();
        file.read_to_end(&mut res)?;

        println!("Loading {:?} at {:#x}...\r", path, offset + self.guest_memory[mem_region_idx].guest_phys_addr);

        self.load_data_to_memory(mem_region_idx, res, offset)
    }

    fn load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
        self.arch_load_linux(vcpu, args)
    }

    fn init_mmio_devices(&mut self) -> io::Result<()> {
        self.arch_init_mmio_devices()
    }

    fn init_io_devices(&mut self) -> io::Result<()> {
        self.arch_init_io_devices()
    }

    fn handle_mmio(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        self.arch_handle_mmio(mmio)
    }

    fn handle_io(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        self.arch_handle_io(io, base)
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

pub struct VCPU {
    pub fd: libc::c_int,
    pub kvm_run_mem: *mut libc::c_void
}

impl VCPU {
    fn set_kvm_run_mem(&mut self, kvm_run_size: usize) -> io::Result<()> {
        // 140904 mmap(NULL, 12288, 0x3 /* PROT_READ|PROT_WRITE */, 0x1 /* MAP_SHARED */, 10<anon_inode:kvm-vcpu:0>, 0) = 0x7769050b0000
        self.kvm_run_mem = unsafe {
            libc::mmap(std::ptr::null_mut(),
                       kvm_run_size as usize,
                       libc::PROT_READ|libc::PROT_WRITE,
                       libc::MAP_SHARED,
                       self.fd,
                       0)
        };
        if self.kvm_run_mem == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for VCPU {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

extern "C" fn handler(_sig: libc::c_int) {
    SHOULD_STOP.store(true, Ordering::SeqCst);
}

unsafe fn install_interrupt_signal() {
    let mut sa: libc::sigaction = std::mem::zeroed();
    sa.sa_sigaction = handler as *const() as usize;
    libc::sigemptyset(&mut sa.sa_mask);
    sa.sa_flags = 0;
    libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let kvm_dev = KvmDev::new()?;
    let mut vm = kvm_dev.create_vm()?;

    arch::arch_pre_vcpu_init(&kvm_dev, &mut vm)?;

    let mut vcpu = vm.create_vcpu()?;

    println!("\nKVM Rust\r");

    arch::arch_init(&kvm_dev, &mut vm, &mut vcpu)?;

    vm.init_mmio_devices()?;
    vm.init_io_devices()?;
    vcpu.set_ip(args.load_addr as usize)?;

    if let Some(_) = args.linux {
        vm.load_linux(&mut vcpu, &args).unwrap();
    } else if let Some(binary) = args.binary {
        let mem_region_idx = vm.add_mem_region(args.memory * 1024, 0x0)?;
        vm.load_file_to_memory(mem_region_idx, &binary, args.load_addr)?;
    }

    vcpu.set_kvm_run_mem(kvm_dev.get_kvm_run_size())?;

    MAIN_TID.store(unsafe { libc::pthread_self() } as u64, Ordering::SeqCst);
    unsafe { install_interrupt_signal() };

    let run = vcpu.kvm_run_mem as *mut kvm_run;

    loop {
        let ret = unsafe { libc::ioctl(vcpu.fd, KVM_RUN, 0usize) };
        if ret < 0 {
            if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                if SHOULD_STOP.load(Ordering::Relaxed) { break; }
                continue;
            }
            vcpu.print_regs()?;
            return Err(io::Error::last_os_error());
        }

        let exit_reason = unsafe { (*run).exit_reason };

        match exit_reason {
            KVM_EXIT_IO => {
                let io = unsafe { (*run).__bindgen_anon_1.io };
                vm.handle_io(&io, vcpu.kvm_run_mem as *mut u8)?;
            }
            KVM_EXIT_SHUTDOWN => {
                println!("Guest shutdown.");
                vcpu.print_regs()?;
                break; }
            KVM_EXIT_MMIO => {
                let mmio = unsafe { &mut (*run).__bindgen_anon_1.mmio };
                vm.handle_mmio(mmio)?
            }
            KVM_EXIT_HLT => {
                println!("Guest halted.");
                break; }
            KVM_EXIT_INTERNAL_ERROR => {
                let internal = unsafe { (*run).__bindgen_anon_1.internal };
                println!("suberror: {:#x}", internal.suberror);
                for i in 0..internal.ndata {
                    println!("{:4}: {:#x}", i, internal.data[i as usize]);
                }
                return Err(io::Error::other("KVM internal error."));
            }
            KVM_EXIT_FAIL_ENTRY => {
                let fail_entry = unsafe { (*run).__bindgen_anon_1.fail_entry };
                let reason = fail_entry.hardware_entry_failure_reason;
                let cpu = fail_entry.cpu;
                println!("Fail entry: reason={:#x}, cpu={}", reason, cpu);
                break;
            }
            _ => {
                println!("EXIT REASON = {}", exit_reason);
                break;
            }
        }
    }

    Ok(())
}
