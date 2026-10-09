use crate::{ kvm_segment, kvm_cpuid2, kvm_cpuid_entry2, kvm_pit_config };
use crate::{ VCPU, VM, KvmDev, Args, KVMIO };
use crate::{ read_le64, read_le32, load_elf };
use libc::{_IOW, _IO, _IOR, _IOWR};

use object::elf;
use object::read::elf::{ElfFile64, SectionHeader};
use object::{Endianness, Object, ObjectSection};
use std::io::{self, Read};

const KVM_CREATE_IRQCHIP: libc::Ioctl = _IO(KVMIO, 0x60);
const KVM_CREATE_PIT2: libc::Ioctl = _IOW::<kvm_pit_config>(KVMIO, 0x77);
const KVM_GET_SUPPORTED_CPUID: libc::Ioctl = _IOWR::<kvm_cpuid2>(KVMIO, 0x05);
const KVM_SET_CPUID2: libc::Ioctl = _IOW::<kvm_cpuid2>(KVMIO, 0x90);

#[repr(C)]
struct HvmStartInfo {
    magic: u32,
    version: u32,
    flags: u32,
    nr_modules: u32,
    modlist_paddr: u64,
    cmdline_paddr: u64,
    rsdp_paddr: u64,
    memmap_paddr: u64,
    memmap_entries: u32,
    reserved: u32
}

#[repr(C)]
struct HvmModlistEntry {
    paddr: u64,
    size: u64,
    cmdline_paddr: u64,
    reserved: u64
}

#[repr(C)]
struct HvmMemmapEntry {
    addr: u64,
    size: u64,
    type_: u32,
    reserved: u32
}

pub fn arch_pre_vcpu_init(kvm_dev: &KvmDev, vm: &mut VM) -> io::Result<()> {
    let ret = unsafe { libc::ioctl(vm.fd, KVM_CREATE_IRQCHIP, 0x0) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    let pit_config = kvm_pit_config { flags: 0, pad: [0; 15] };

    let ret = unsafe { libc::ioctl(vm.fd, KVM_CREATE_PIT2, &pit_config) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

pub fn arch_init(kvm_dev: &KvmDev, vm: &mut VM, vcpu: &mut VCPU) -> io::Result<()> {
    let mut cpuid2 = vec![0u32; size_of::<kvm_cpuid2>() + 256 * size_of::<kvm_cpuid_entry2>()].as_mut_ptr() as *mut kvm_cpuid2;
    unsafe { (*cpuid2).nent = 256; }

    let ret = unsafe { libc::ioctl(kvm_dev.fd(), KVM_GET_SUPPORTED_CPUID, cpuid2) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    let nent = unsafe { (*cpuid2).nent as usize };
    let entries = unsafe { (*cpuid2).__bindgen_anon_1.entries.as_slice(nent) };

    for i in 0..nent {
        let entry = entries[i as usize];
        println!("{:#x} {:#x}", entry.function, entry.index);
    }

    let ret = unsafe { libc::ioctl(vcpu.fd, KVM_SET_CPUID2, cpuid2) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut sregs2 = vcpu.get_sregs2()?;

    sregs2.cs.base = 0;
    sregs2.cs.selector = 0;

    vcpu.set_sregs2(sregs2)
}

impl VM {
    fn try_pvh_entrypoint(&mut self, vmlinux: &ElfFile64<Endianness>) -> io::Result<u64> {
        let endian = vmlinux.endian();
        let mut notes = vmlinux.section_by_name(".notes").ok_or(io::Error::other("No .notes found."))?
            .elf_section_header()
            .notes(endian, vmlinux.data()).unwrap()
            .unwrap();

        while let Some(note) = notes.next().unwrap() {
            if note.n_type(endian) == object::elf::NoteType(18) {
                return match note.desc().len() {
                    4 => Ok(read_le32(note.desc(), 0) as u64),
                    8 => Ok(read_le64(note.desc(), 0)),
                    _ => Err(io::Error::other(format!("unexpected PHYS32_ENTRY descsz {}", note.desc().len())))
                };
            }
        }

        Err(io::Error::other(format!("No valid PVH entry found.")))
    }

    fn linux_pvh_boot(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
        let vmlinux_data = std::fs::read(&args.linux.as_ref().unwrap())?;
        let vmlinux = ElfFile64::<Endianness>::parse(&*vmlinux_data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let pvh_entrypoint = self.try_pvh_entrypoint(&vmlinux)?;

        let ram_size: u64 = 1024 * 1024 * 1024;
        let linux_mem_idx = self.add_mem_region(ram_size as usize, 0x0)?;

        let last_addr = load_elf(self, &vmlinux, &vmlinux_data, linux_mem_idx);

        let mut cmdline = String::from("console=ttyS0,115200 root=/dev/vda virtio_mmio.device=4K@0xd0000000:5");
        let mut modlist_entries = vec![];

        let hvm_base: u64 = 0x10000;

        match &args.initramfs {
            Some(initramfs_path) => {
                let initrd_cmdline = "\0";
                let initrd_cmdline_start = (last_addr + 0xfff) & !0xfff;
                self.load_data_to_memory(linux_mem_idx, initrd_cmdline.as_bytes().to_vec(), initrd_cmdline_start);

                let initrd_start = ((initrd_cmdline.len() as u64) + initrd_cmdline_start + 0xfff) & !0xfff;
                let initramfs_size = self.load_file_to_memory(linux_mem_idx, &initramfs_path, initrd_start).unwrap();
                modlist_entries.push(HvmModlistEntry {
                    paddr: initrd_start as u64,
                    size: initramfs_size as u64,
                    cmdline_paddr: initrd_cmdline_start as u64,
                    reserved: 0u64
                });
            },
            None => ()
        }

        const EBDA_START: u64 = 0x9fc00;
        const HIMEM_START: u64 = 0x10_0000;
        const MMIO_START: u64 = 0xc000_0000;
        const HIGH_RAM_START: u64 = 0x1_0000_0000;

        let mut memmap_entries = vec![
            HvmMemmapEntry { addr: 0x0, size: EBDA_START, type_: 1, reserved: 0 },
            HvmMemmapEntry { addr: EBDA_START, size: HIMEM_START - EBDA_START, type_: 2, reserved: 0 },
            HvmMemmapEntry { addr: HIMEM_START, size: ram_size.min(MMIO_START) - HIMEM_START, type_: 1, reserved: 0}
        ];
        if ram_size > MMIO_START {
            /* needs another slot alloc */
            memmap_entries.push(HvmMemmapEntry { addr: HIGH_RAM_START, size: ram_size - MMIO_START, type_: 1, reserved: 0 });
        }

        let mut cmdline_start = hvm_base + std::mem::size_of::<HvmStartInfo>() as u64;
        cmdline_start = (cmdline_start + 0xfff) & !0xfff;
        let cmdline_size = cmdline.len() as u64;
        self.load_data_to_memory(linux_mem_idx, cmdline.as_bytes().to_vec(), cmdline_start);

        let mut memmap_start = cmdline_start + cmdline_size;
        memmap_start = (memmap_start + 0xfff) & !0xfff;
        let memmap_size = memmap_entries.len() * std::mem::size_of::<HvmMemmapEntry>();
        let mut memmap_buf = Vec::with_capacity(memmap_size);
        for e in &memmap_entries {
            memmap_buf.extend_from_slice(&e.addr.to_le_bytes());
            memmap_buf.extend_from_slice(&e.size.to_le_bytes());
            memmap_buf.extend_from_slice(&e.type_.to_le_bytes());
            memmap_buf.extend_from_slice(&e.reserved.to_le_bytes());
        }
        self.load_data_to_memory(linux_mem_idx, memmap_buf, memmap_start);

        let mut modlist_start = memmap_start + memmap_size as u64;
        modlist_start = (modlist_start + 0xfff) & !0xfff;
        let modlist_size = modlist_entries.len() * std::mem::size_of::<HvmModlistEntry>();
        let mut modlist_buf = Vec::with_capacity(modlist_size);
        for e in &modlist_entries {
            modlist_buf.extend_from_slice(&e.paddr.to_le_bytes());
            modlist_buf.extend_from_slice(&e.size.to_le_bytes());
            modlist_buf.extend_from_slice(&e.cmdline_paddr.to_le_bytes());
            modlist_buf.extend_from_slice(&e.reserved.to_le_bytes());
        }
        self.load_data_to_memory(linux_mem_idx, modlist_buf, modlist_start);

        let mut magic = [ 'x', 'E', 'n', '3' ].map(|c| c as u8); magic[1] |= 0x80;
        let mut hvm_start_info = HvmStartInfo {
            magic: read_le32(&magic, 0x0),
            version: 1u32,
            flags: 0u32,
            nr_modules: modlist_entries.len() as u32,
            modlist_paddr: modlist_start as u64,
            cmdline_paddr: cmdline_start as u64,
            rsdp_paddr: 0u64,
            memmap_paddr: memmap_start as u64,
            memmap_entries: memmap_entries.len() as u32,
            reserved: 0u32
        };
        let hvm_mem_src = unsafe { std::slice::from_raw_parts(
            &hvm_start_info as *const HvmStartInfo as *const u8,
            std::mem::size_of::<HvmStartInfo>(),
        ) };
        self.load_data_to_memory(linux_mem_idx, hvm_mem_src.to_vec(), hvm_base);

        let mut regs = vcpu.get_regs()?;
        regs.rip = pvh_entrypoint;
        regs.rbx = hvm_base;
        regs.rflags = 0x2; // bits 17, 9 and 8 cleared, all other unspecified
        vcpu.set_regs(regs);
        regs.print();

        vcpu.protected_mode_setup();

        Ok(())
    }

    pub fn arch_load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
        self.linux_pvh_boot(vcpu, args)
    }
}
