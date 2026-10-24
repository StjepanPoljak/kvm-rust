#[allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code)]
mod kvm_bindings {
    include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));
}
use kvm_bindings::*;

include!("frontend/mod.rs");
include!("util/mod.rs");
include!("arch/mod.rs");
include!("virtio/mod.rs");

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use clap::Parser;
use std::fs::File;
use std::io::{ self, Read };
use libc::{ _IOW, _IO };
use std::collections::HashMap;
use std::sync::{ Arc,Mutex };
use std::sync::atomic::{ AtomicBool, Ordering };
use std::cell::Cell;

type MMIO = kvm_run__bindgen_ty_1__bindgen_ty_6;
type IO = kvm_run__bindgen_ty_1__bindgen_ty_4;
type MMIODevices = HashMap::<u64, Arc<Mutex<dyn MMIODevice>>>;
type IODevices = HashMap::<u64, Arc<Mutex<dyn IODevice>>>;
/* Shared, so a device reading guest RAM from a frontend thread keeps
 * its region mapped after the VM is gone. */
type GuestMemory = Vec<Arc<MemRegion>>;

pub trait MMIODevice: Send {
    fn handle(&mut self, mmio: &mut MMIO, mem: &GuestMemory) -> io::Result<()>;
    /* Picks up work that arrived from outside the guest, see VmControl::kick(). */
    fn poll(&mut self, _mem: &GuestMemory) -> io::Result<()> { Ok(()) }
}

pub trait IODevice: Send {
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()>;
}

thread_local! {
    /* The kvm_run of the vcpu running on this thread, for kick_handler(). */
    static KVM_RUN_PTR: Cell<*mut kvm_run> = const { Cell::new(std::ptr::null_mut()) };
}

/* Per-VM state shared by its vcpu threads and device threads. */
pub struct VmControl {
    should_stop: AtomicBool,
    devices_pending: AtomicBool,
    /* Threads currently in VCPU::run(), the targets of kick_signal(). */
    vcpu_threads: Mutex<Vec<libc::pthread_t>>
}

impl VmControl {
    fn new() -> Self {
        Self {
            should_stop: AtomicBool::new(false),
            devices_pending: AtomicBool::new(false),
            vcpu_threads: Mutex::new(vec![])
        }
    }

    /* Makes every vcpu thread leave its run loop. */
    pub fn stop(&self) {
        self.should_stop.store(true, Ordering::SeqCst);
        for tid in self.vcpu_threads.lock().unwrap().iter() {
            unsafe { libc::pthread_kill(*tid, kick_signal()); }
        }
    }

    /* Called from device threads: makes a vcpu thread poll the MMIO devices. */
    pub fn kick(&self) {
        self.devices_pending.store(true, Ordering::SeqCst);
        if let Some(tid) = self.vcpu_threads.lock().unwrap().first() {
            unsafe { libc::pthread_kill(*tid, kick_signal()); }
        }
    }

    fn should_stop(&self) -> bool {
        self.should_stop.load(Ordering::SeqCst)
    }

    fn take_devices_pending(&self) -> bool {
        self.devices_pending.swap(false, Ordering::SeqCst)
    }

    /* Kicks only reach registered threads, and a thread is unregistered
     * under the same lock, so pthread_kill never sees a stale thread id. */
    fn register_vcpu_thread(&self) {
        self.vcpu_threads.lock().unwrap().push(unsafe { libc::pthread_self() });
    }

    fn unregister_vcpu_thread(&self) {
        let tid = unsafe { libc::pthread_self() };
        self.vcpu_threads.lock().unwrap().retain(|t| unsafe { libc::pthread_equal(*t, tid) } == 0);
    }
}

const KVM_CREATE_VM: libc::Ioctl = _IO(KVMIO, 0x01);
const KVM_CHECK_EXTENSION: libc::Ioctl = _IO(KVMIO, 0x03);
const KVM_CREATE_VCPU: libc::Ioctl = _IO(KVMIO, 0x41);
const KVM_GET_VCPU_MMAP_SIZE: libc::Ioctl = _IO(KVMIO, 0x04);

const KVM_SET_USER_MEMORY_REGION: libc::Ioctl = _IOW::<kvm_userspace_memory_region>(KVMIO, 0x46);
const KVM_GET_DIRTY_LOG: libc::Ioctl = _IOW::<kvm_dirty_log>(KVMIO, 0x42);
const KVM_REGISTER_COALESCED_MMIO: libc::Ioctl = _IOW::<kvm_coalesced_mmio_zone>(KVMIO, 0x67);
const PAGE_SIZE: u64 = 4096;
const KVM_RUN: libc::Ioctl = _IO(KVMIO, 0x80);

#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Args {
    /// Binary to run
    #[arg(short, long)]
    binary: Option<String>,

    /// Kernel to boot: a vmlinux or other PVH ELF on x86, an Image on aarch64
    #[arg(long, alias = "kernel")]
    linux: Option<String>,

    /// Memory size: a number with a K, M or G suffix; a plain number is
    /// in kilobytes [default: 256K for --binary, 1G for --linux]
    #[arg(short, long, value_parser = parse_mem_size)]
    memory: Option<usize>,

    /// Load address
    #[arg(short, long, default_value_t = 0x1000)]
    load_addr: u64,

    /// Device tree blob
    #[arg(long)]
    dtb: Option<String>,

    /// Initramfs, or on x86 any file to pass as a PVH module
    #[arg(long)]
    initramfs: Option<String>,

    /// Disk path
    #[arg(long)]
    disk: Option<String>,

    /// Tap interface name
    #[arg(long)]
    tap: Option<String>,

    /// Extra kernel command line arguments (x86; on aarch64 the DTB has them)
    #[arg(long, allow_hyphen_values = true)]
    append: Option<String>,

    /// Serve the display and keyboard to VNC clients on this address,
    /// e.g. 127.0.0.1:5900. No authentication: keep it local or tunnel it.
    #[arg(long)]
    vnc: Option<String>
}

/* Returns the size in bytes. */
fn parse_mem_size(arg: &str) -> Result<usize, String> {
    let (num, unit) = match arg.char_indices().last() {
        Some((i, c)) if c.is_ascii_alphabetic() => (&arg[..i], c.to_ascii_uppercase()),
        _ => (arg, 'K')
    };
    let shift = match unit {
        'K' => 10,
        'M' => 20,
        'G' => 30,
        _ => return Err(format!("unknown size suffix '{}'", unit))
    };
    let num: usize = num.parse().map_err(|e| format!("{}", e))?;
    num.checked_mul(1 << shift)
        .filter(|size| *size > 0)
        .ok_or(format!("invalid memory size {}", arg))
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
            next_slot: 0,
            mmio_devices: MMIODevices::new(),
            io_devices: IODevices::new(),
            display: None,
            keyboard: None,
            control: Arc::new(VmControl::new()) })
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
    next_slot: u32,
    pub mmio_devices: MMIODevices,
    pub io_devices: IODevices,
    /* Set by the devices that provide them, for frontends such as RFB. */
    pub display: Option<Arc<dyn Display>>,
    pub keyboard: Option<Arc<Mutex<dyn Keyboard>>>,
    pub control: Arc<VmControl>
}

pub struct MemRegion {
    pub mem_ptr: *mut libc::c_void,
    pub mem_size: usize,
    pub guest_phys_addr: u64,
    /* The KVM slot covering all of it; None once log_dirty_pages has
     * split it into several. */
    slot: Option<u32>
}

/* Which pages of a memory slot the guest has written since the last
 * fetch. KVM write-protects the slot's pages and notes each one on its
 * first write (or, on Intel, the CPU logs it with PML). */
pub struct DirtyLog {
    vm_fd: libc::c_int,
    slot: u32,
    pages: usize
}

impl DirtyLog {
    /* A bit per page, set if the page was written; fetching clears the
     * log, so a write made after this returns is in the next one. */
    pub fn fetch(&self) -> io::Result<Vec<u64>> {
        /* KVM copies whole longs. */
        let mut bitmap = vec![0u64; self.pages.div_ceil(64)];
        let log = kvm_dirty_log {
            slot: self.slot,
            padding1: 0,
            __bindgen_anon_1: kvm_dirty_log__bindgen_ty_1 { dirty_bitmap: bitmap.as_mut_ptr().cast() }
        };
        let ret = unsafe { libc::ioctl(self.vm_fd, KVM_GET_DIRTY_LOG, &log) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(bitmap)
    }
}

impl Drop for DirtyLog {
    fn drop(&mut self) {
        unsafe { libc::close(self.vm_fd); }
    }
}

/* Writes KVM queued instead of exiting to us, see VM::coalesce_pio().
 * KVM appends at `last` and the reader frees entries by advancing
 * `first`; when the ring is full, a write exits as usual. The ring is
 * the VM's, so it has one reader, which must apply what is queued
 * before handling an exit for the same ports, or writes reorder. */
pub struct CoalescedRing {
    ring: *mut kvm_coalesced_mmio_ring
}

/* Only the reader touches `first`, and it is behind the device's lock. */
unsafe impl Send for CoalescedRing {}

impl CoalescedRing {
    const ENTRIES: u32 = ((PAGE_SIZE as usize - std::mem::size_of::<kvm_coalesced_mmio_ring>())
                          / std::mem::size_of::<kvm_coalesced_mmio>()) as u32;

    /* Calls write(port, data) for each queued write, oldest first. */
    pub fn drain(&self, mut write: impl FnMut(u64, &[u8])) {
        let first = unsafe { std::ptr::addr_of_mut!((*self.ring).first) };
        let last = unsafe { std::ptr::addr_of!((*self.ring).last) };
        let entries = unsafe { (*self.ring).__bindgen_anon_1.coalesced_mmio.as_ptr() };
        let mut i = read_guest_ptr::<u32>(first.cast());
        /* Entries up to `last` are complete once it is read. */
        let end = read_guest_ptr::<u32>(last.cast());
        std::sync::atomic::fence(Ordering::Acquire);
        while i != end {
            let entry = unsafe { std::ptr::read_volatile(entries.add(i as usize)) };
            write(entry.phys_addr, &entry.data[..(entry.len as usize).min(entry.data.len())]);
            i = (i + 1) % Self::ENTRIES;
        }
        /* KVM may reuse the entries once `first` passes them. */
        std::sync::atomic::fence(Ordering::Release);
        write_guest_ptr::<u32>(first.cast(), end);
    }
}

impl Drop for CoalescedRing {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ring.cast(), PAGE_SIZE as usize); }
    }
}

/* The mapping lives as long as the last Arc to the region, and the
 * guest writes it concurrently anyway; accesses go through volatile reads and writes. */
unsafe impl Send for MemRegion {}
unsafe impl Sync for MemRegion {}

impl Drop for MemRegion {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.mem_ptr, self.mem_size); }
    }
}

fn gpa_to_host(mem: &GuestMemory, gpa: u64, len: usize) -> Option<*mut u8> {
    mem.iter().find_map(|r| {
        let off = usize::try_from(gpa.checked_sub(r.guest_phys_addr)?).ok()?;
        (off.checked_add(len)? <= r.mem_size).then(|| unsafe { (r.mem_ptr as *mut u8).add(off) })
    })
}

fn read_guest_ptr<T>(ptr: *const u8) -> T {
    unsafe {
        std::ptr::read_volatile(ptr.cast::<T>())
    }
}

fn write_guest_ptr<T>(ptr: *mut u8, data: T) {
    unsafe {
        std::ptr::write_volatile(ptr.cast::<T>(), data)
    }
}

impl VM {
    fn create_vcpu(&self, id: u32, kvm_run_size: usize) -> io::Result<VCPU> {
        // 140904 ioctl(9<anon_inode:kvm-vm>, 0xae41 /* KVM_CREATE_VCPU */, 0) = 10<anon_inode:kvm-vcpu:0>
        let vcpu_fd = unsafe { libc::ioctl(self.fd, KVM_CREATE_VCPU, id as usize) };
        if vcpu_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let mut vcpu = VCPU { id, fd: vcpu_fd, kvm_run_mem: std::ptr::null_mut(), kvm_run_size };
        vcpu.map_kvm_run()?;
        Ok(vcpu)
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

        let slot = self.next_slot;
        self.next_slot += 1;
        self.set_user_memory_region(slot, 0, guest_phys_addr, mem_size as u64, mem_ptr as u64)?;
        self.guest_memory.push(Arc::new(MemRegion { mem_ptr, mem_size, guest_phys_addr, slot: Some(slot) }));

        Ok(self.guest_memory.len() - 1)
    }

    /* A memory_size of 0 deletes the slot. */
    fn set_user_memory_region(&self, slot: u32, flags: u32, guest_phys_addr: u64,
                              memory_size: u64, userspace_addr: u64) -> io::Result<()> {
        // 140900 ioctl(9<anon_inode:kvm-vm>, 0x4020ae46 /* KVM_SET_USER_MEMORY_REGION */, {slot=0, flags=0, guest_phys_addr=0, memory_size=1073741824, userspace_addr=0x7768b3e00000}) = 0
        let region = kvm_userspace_memory_region { slot, flags, guest_phys_addr, memory_size, userspace_addr };
        let ret = unsafe { libc::ioctl(self.fd, KVM_SET_USER_MEMORY_REGION, &region) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /* Starts logging writes to the pages covering gpa..gpa + size. Logging
     * is per slot, and a slot cannot be resized, so the region's slot is
     * deleted and added again as up to three: before, logged, after. The
     * mapping stays one piece. Once per region, before the vcpus run. */
    pub fn log_dirty_pages(&mut self, gpa: u64, size: usize) -> io::Result<DirtyLog> {
        let start = gpa & !(PAGE_SIZE - 1);
        let end = (gpa + size as u64).next_multiple_of(PAGE_SIZE);
        let idx = self.guest_memory.iter()
            .position(|r| r.guest_phys_addr <= start && end <= r.guest_phys_addr + r.mem_size as u64)
            .ok_or(io::Error::other(format!("No memory region covers {:#x}..{:#x}.", start, end)))?;
        let region = &self.guest_memory[idx];
        let (base, host) = (region.guest_phys_addr, region.mem_ptr as u64);
        let region_end = base + region.mem_size as u64;
        let slot = region.slot.ok_or(io::Error::other("Memory region is already split."))?;

        self.set_user_memory_region(slot, 0, base, 0, host)?;
        let mut logged = None;
        let mut free_slot = Some(slot);
        for (from, to, flags) in [(base, start, 0), (start, end, KVM_MEM_LOG_DIRTY_PAGES), (end, region_end, 0)] {
            if from == to {
                continue;
            }
            let slot = free_slot.take().unwrap_or_else(|| {
                self.next_slot += 1;
                self.next_slot - 1
            });
            self.set_user_memory_region(slot, flags, from, to - from, host + (from - base))?;
            if flags != 0 {
                logged = Some(slot);
            }
        }
        Arc::get_mut(&mut self.guest_memory[idx])
            .ok_or(io::Error::other("Memory region is already shared."))?
            .slot = None;

        /* Its own fd, as frontends may fetch it after the VM is dropped. */
        let vm_fd = unsafe { libc::dup(self.fd) };
        if vm_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(DirtyLog { vm_fd, slot: logged.unwrap(), pages: ((end - start) / PAGE_SIZE) as usize })
    }

    /* Makes KVM queue the guest's writes to ports..ports + len in the
     * returned ring rather than exit for each; reads still exit. None
     * if KVM can't. Once per VM: the ring is shared by every zone. It
     * is mapped through a vcpu, and on its own, so a frontend can keep
     * reading it after the vcpus are gone. */
    pub fn coalesce_pio(&self, vcpu: &VCPU, port: u16, len: u32) -> io::Result<Option<CoalescedRing>> {
        let supported = unsafe { libc::ioctl(self.fd, KVM_CHECK_EXTENSION, KVM_CAP_COALESCED_PIO as usize) };
        if supported <= 0 {
            return Ok(None);
        }
        let zone = kvm_coalesced_mmio_zone {
            addr: port as u64,
            size: len,
            __bindgen_anon_1: kvm_coalesced_mmio_zone__bindgen_ty_1 { pio: 1 }
        };
        let ret = unsafe { libc::ioctl(self.fd, KVM_REGISTER_COALESCED_MMIO, &zone) };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
        let ring = unsafe {
            libc::mmap(std::ptr::null_mut(),
                       PAGE_SIZE as usize,
                       libc::PROT_READ|libc::PROT_WRITE,
                       libc::MAP_SHARED,
                       vcpu.fd,
                       (KVM_COALESCED_MMIO_PAGE_OFFSET as u64 * PAGE_SIZE) as libc::off_t)
        };
        if ring == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(CoalescedRing { ring: ring.cast() }))
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

    fn init_mmio_devices(&mut self, args: &Args) -> io::Result<()> {
        self.arch_init_mmio_devices(args)
    }

    fn init_io_devices(&mut self, vcpu: &VCPU) -> io::Result<()> {
        self.arch_init_io_devices(vcpu)
    }

    fn add_mmio_device(&mut self, addr: u64, device: Arc<Mutex<dyn MMIODevice>>) {
        self.mmio_devices.insert(addr >> 12, device);
    }

    fn handle_mmio(&self, mmio: &mut MMIO) -> io::Result<()> {
        let page = mmio.phys_addr >> 12;
        if let Some(device) = self.mmio_devices.get(&page) {
            device.lock().unwrap().handle(mmio, &self.guest_memory)?;
        }
        Ok(())
    }

    fn handle_io(&self, io: &IO, base: *mut u8) -> io::Result<()> {
        self.arch_handle_io(io, base)
    }

    fn poll_mmio_devices(&self) -> io::Result<()> {
        for device in self.mmio_devices.values() {
            device.lock().unwrap().poll(&self.guest_memory)?;
        }
        Ok(())
    }

    /* Runs each vcpu on its own thread until the VM stops. The first vcpu
     * to leave its run loop stops the others; the first error is returned. */
    fn run(self, vcpus: Vec<VCPU>) -> io::Result<()> {
        unsafe { install_kick_handler() };

        let vm = Arc::new(self);
        let mut threads = vec![];
        let mut res = Ok(());
        for mut vcpu in vcpus {
            let vm_thread = vm.clone();
            let thread = std::thread::Builder::new()
                .name(format!("vcpu{}", vcpu.id))
                .spawn(move || {
                    let res = vcpu.run(&vm_thread);
                    vm_thread.control.stop();
                    res
                });
            match thread {
                Ok(thread) => threads.push(thread),
                Err(e) => { vm.control.stop(); res = Err(e); break; }
            }
        }

        for thread in threads {
            let thread_res = thread.join()
                .unwrap_or_else(|_| Err(io::Error::other("vCPU thread panicked.")));
            if res.is_ok() {
                res = thread_res;
            }
        }
        res
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

pub struct VCPU {
    pub id: u32,
    pub fd: libc::c_int,
    pub kvm_run_mem: *mut libc::c_void,
    kvm_run_size: usize
}

/* kvm_run is only touched by the thread running the vcpu. */
unsafe impl Send for VCPU {}

impl VCPU {
    fn map_kvm_run(&mut self) -> io::Result<()> {
        // 140904 mmap(NULL, 12288, 0x3 /* PROT_READ|PROT_WRITE */, 0x1 /* MAP_SHARED */, 10<anon_inode:kvm-vcpu:0>, 0) = 0x7769050b0000
        self.kvm_run_mem = unsafe {
            libc::mmap(std::ptr::null_mut(),
                       self.kvm_run_size,
                       libc::PROT_READ|libc::PROT_WRITE,
                       libc::MAP_SHARED,
                       self.fd,
                       0)
        };
        if self.kvm_run_mem == libc::MAP_FAILED {
            self.kvm_run_mem = std::ptr::null_mut();
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /* Runs the vcpu on the calling thread until it exits or the VM stops. */
    fn run(&mut self, vm: &VM) -> io::Result<()> {
        KVM_RUN_PTR.set(self.kvm_run_mem as *mut kvm_run);
        vm.control.register_vcpu_thread();
        let res = self.run_loop(vm);
        vm.control.unregister_vcpu_thread();
        KVM_RUN_PTR.set(std::ptr::null_mut());
        res
    }

    fn run_loop(&mut self, vm: &VM) -> io::Result<()> {
        let run = self.kvm_run_mem as *mut kvm_run;

        loop {
            if vm.control.should_stop() {
                break;
            }
            if vm.control.take_devices_pending() {
                vm.poll_mmio_devices()?;
            }

            let ret = unsafe { libc::ioctl(self.fd, KVM_RUN, 0usize) };
            if ret < 0 {
                if io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    unsafe { std::ptr::write_volatile(&mut (*run).immediate_exit, 0); }
                    continue;
                }
                self.print_regs()?;
                return Err(io::Error::last_os_error());
            }

            let exit_reason = unsafe { (*run).exit_reason };

            match exit_reason {
                KVM_EXIT_IO => {
                    let io = unsafe { (*run).__bindgen_anon_1.io };
                    vm.handle_io(&io, self.kvm_run_mem as *mut u8)?;
                }
                KVM_EXIT_SHUTDOWN => {
                    println!("Guest shutdown.");
                    self.print_regs()?;
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
}

impl Drop for VCPU {
    fn drop(&mut self) {
        if !self.kvm_run_mem.is_null() {
            unsafe { libc::munmap(self.kvm_run_mem, self.kvm_run_size); }
        }
        unsafe { libc::close(self.fd); }
    }
}

/* immediate_exit makes KVM_RUN return EINTR even if the kick lands before the vcpu enters the guest. */
extern "C" fn kick_handler(_sig: libc::c_int) {
    let run = KVM_RUN_PTR.get();
    if !run.is_null() {
        unsafe { std::ptr::write_volatile(&mut (*run).immediate_exit, 1); }
    }
}

/* Real-time signal, so it can't clash with SIGUSR1/2 users. */
fn kick_signal() -> libc::c_int {
    libc::SIGRTMIN()
}

/* Must run before any vcpu thread registers: the default action would kill us. */
unsafe fn install_kick_handler() {
    let mut sa: libc::sigaction = std::mem::zeroed();
    sa.sa_sigaction = kick_handler as *const() as usize;
    libc::sigemptyset(&mut sa.sa_mask);
    sa.sa_flags = 0;
    libc::sigaction(kick_signal(), &sa, std::ptr::null_mut());
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let kvm_dev = KvmDev::new()?;
    let mut vm = kvm_dev.create_vm()?;

    arch::arch_pre_vcpu_init(&kvm_dev, &mut vm)?;

    let mut vcpu = vm.create_vcpu(0, kvm_dev.get_kvm_run_size())?;

    println!("\nHearth\r");

    arch::arch_init(&kvm_dev, &mut vm, &mut vcpu)?;

    vcpu.set_ip(args.load_addr as usize)?;

    /* Guest memory is complete before any device (and device thread) exists. */
    if let Some(_) = args.linux {
        vm.load_linux(&mut vcpu, &args).unwrap();
    } else if let Some(binary) = &args.binary {
        let mem_region_idx = vm.add_mem_region(args.memory.unwrap_or(256 << 10), 0x0)?;
        vm.load_file_to_memory(mem_region_idx, binary, args.load_addr)?;
    }

    /* The serial frontend's thread may never get to restore the terminal
     * when the guest stops the VM, so main restores it. */
    let saved_tty = save_tty();

    vm.init_mmio_devices(&args)?;
    vm.init_io_devices(&vcpu)?;
    if let Some(addr) = &args.vnc {
        start_rfb(addr.as_str(), vm.display.clone(), vm.keyboard.clone())?;
    }

    let res = vm.run(vec![vcpu]);
    if let Some(tty) = saved_tty {
        restore_tty(&tty);
    }
    res
}
