use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::ptr;
use clap::Parser;
use std::fs::File;
use std::io::{self, Read};
use libc::{_IOW, _IO, _IOR};
use std::collections::HashMap;
use std::sync::{Arc,Mutex};
use std::sync::atomic::{AtomicBool, Ordering, AtomicU64};
use std::io::Write;

type MMIO = kvm_run__bindgen_ty_1__bindgen_ty_6;
type MMIODevices = HashMap::<u64, Arc<Mutex<dyn MMIODevice>>>;
const KVM_EXIT_IO_OUT: u8 = 1;
const KVM_EXIT_IO_IN: u8 = 0;
trait MMIODevice {
    fn handle(&mut self, vm_fd: libc::c_int, mmio: &mut MMIO) -> io::Result<()>;
}

static SHOULD_STOP: AtomicBool = AtomicBool::new(false);
static MAIN_TID: AtomicU64 = AtomicU64::new(0);

include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

include!("util.rs");
include!("arch/mod.rs");

pub const KVMIO: u32 = 0xae;

const KVM_CREATE_VM: u64 = _IO(KVMIO, 0x01);
const KVM_CREATE_VCPU: u64 = _IO(KVMIO, 0x41);
const KVM_GET_VCPU_MMAP_SIZE: u64 = _IO(KVMIO, 0x04);

const KVM_SET_USER_MEMORY_REGION: u64 = _IOW::<kvm_userspace_memory_region>(KVMIO, 0x46);
const KVM_RUN: u64 = _IO(KVMIO, 0x80);

const KVM_EXIT_IO: u32 = 2;
const KVM_EXIT_HLT: u32 = 5;
const KVM_EXIT_MMIO: u32 = 6;
const KVM_EXIT_SHUTDOWN: u32 = 8;
const KVM_EXIT_FAIL_ENTRY: u32 = 9;
const KVM_EXIT_INTERNAL_ERROR: u32 = 17;

const KVM_INTERNAL_ERROR_EMULATION: u32 = 1;
const KVM_INTERNAL_ERROR_SIMUL_EX: u32 = 2;
const KVM_INTERNAL_ERROR_DELIVERY_EV: u32 = 3;
const KVM_INTERNAL_ERROR_UNEXPECTED_EXIT_REASON: u32 = 4;


#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Binary to run
    binary: String,

    /// Memory size in kilobytes
    #[arg(short, long, default_value_t = 256)]
    memory: usize,

    /// Load address
    #[arg(short, long, default_value_t = 0x1000)]
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

        Ok(VM { fd: vm_fd, mem_regions: Vec::<MemRegion>::new(), mmio_devices: MMIODevices::new() })
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
    pub mem_regions: Vec<MemRegion>,
    pub mmio_devices: MMIODevices
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
            libc::mmap(ptr::null_mut(),
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
            slot : self.mem_regions.len() as u32,
            flags : 0,
            guest_phys_addr : guest_phys_addr,
            memory_size : mem_size as u64,
            userspace_addr : mem_ptr as u64
        };

        let ret = unsafe { libc::ioctl(self.fd, KVM_SET_USER_MEMORY_REGION, &region) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        self.mem_regions.push(MemRegion { mem_ptr, mem_size, guest_phys_addr });

        Ok(self.mem_regions.len() - 1)
    }

    fn load_data_to_memory(&self, mem_region_idx: usize, data: Vec<u8>, offset: u64) -> io::Result<usize> {
        let mem_region = self.mem_regions.get(mem_region_idx).ok_or(io::Error::other("Index exceeds memory region vector."))?;

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

        println!("Loading {:?} at {:#x}...\r", path, offset + self.mem_regions[mem_region_idx].guest_phys_addr);

        self.load_data_to_memory(mem_region_idx, res, offset)
    }

    fn load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
        self.arch_load_linux(vcpu, args)
    }

    fn init_mmio_devices(&mut self) -> io::Result<()> {
        self.arch_init_mmio_devices()
    }

    fn handle_mmio(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        self.arch_handle_mmio(mmio)
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
            libc::mmap(ptr::null_mut(),
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

    arch::arch_pre_vcpu_init(&kvm_dev, &mut vm);

    let mut vcpu = vm.create_vcpu()?;

    println!("\nKVM Rust\r");

    arch::arch_init(&kvm_dev, &mut vm, &mut vcpu)?;

    vm.init_mmio_devices()?;
    vcpu.set_ip(args.load_addr as usize)?;

    if let Err(e) = vm.load_linux(&mut vcpu, &args) {
        println!("Binary is not a bootable Linux image for this architecture. Attempting raw...\r");
        let mem_region_idx = vm.add_mem_region(args.memory * 1024, 0x0)?;
        vm.load_file_to_memory(mem_region_idx, &args.binary, args.load_addr)?;
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
                let port = io.port;
                let direction = io.direction;
                let size = io.size as usize;
                let count = io.count as usize;
                let offset = io.data_offset as usize;
                let base = vcpu.kvm_run_mem as *mut u8;
                let data = unsafe {
                    std::slice::from_raw_parts_mut(base.add(offset), size * count)
                };
                match (direction, port) {
                    (KVM_EXIT_IO_OUT, 0x3f8) => {
                        print!("{}", data[0] as char);
                        io::stdout().flush().ok();
                    }
                    (KVM_EXIT_IO_IN, 0x3fd) => data[0] = 0x60,   // LSR: THRE | TEMT
                    (KVM_EXIT_IO_IN, 0x3f8..=0x3ff) => data[0] = 0x00,
                    (KVM_EXIT_IO_OUT, _) => {},                   // swallow
                    (KVM_EXIT_IO_IN, _) => data[0] = 0xff,       // no device
                    (_, _) => data[0] = 0xff
                } }
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
