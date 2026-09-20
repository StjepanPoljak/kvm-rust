use crate::{ kvm_segment, kvm_cpuid2, kvm_cpuid_entry2, kvm_pit_config };
use crate::{ VCPU, VM, KvmDev, Args, KVMIO };
use crate::{ read_le64, read_le32 };
use libc::{_IOW, _IO, _IOR, _IOWR};

use object::elf;
use object::read::elf::{ElfFile64, ProgramHeader, SectionHeader};
use object::{Endianness, Object, ObjectSection};

use std::io::{self, Read};
const KVM_CREATE_IRQCHIP: u64 = _IO(KVMIO, 0x60);
const KVM_CREATE_PIT2: u64 = _IOW::<kvm_pit_config>(KVMIO, 0x77);
const KVM_GET_SUPPORTED_CPUID: u64 = _IOWR::<kvm_cpuid2>(KVMIO, 0x05);
const KVM_SET_CPUID2: u64 = _IOW::<kvm_cpuid2>(KVMIO, 0x90);

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

impl VM {
    fn try_pvh_entrypoint(&mut self, vmlinux: &ElfFile64<Endianness>) -> io::Result<u64> {
        let endian = vmlinux.endian();
        let mut notes = vmlinux.section_by_name(".notes").unwrap()
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
        let vmlinux_data = std::fs::read(&args.binary)?;
        let vmlinux = ElfFile64::<Endianness>::parse(&*vmlinux_data).unwrap();
        let endian = vmlinux.endian();

        let pvh_entrypoint = self.try_pvh_entrypoint(&vmlinux)?;

        let mut addr_start: u64 = 0x0;
        let mut addr_end: u64 = 0x0;
        let mut size: u64 = 0x0;

        for ph in vmlinux.elf_program_headers() {
            if ph.p_type(endian) != elf::PT_LOAD {
                continue;
            }
            let paddr = ph.p_paddr(endian);
            let size = ph.p_memsz(endian);
            if addr_start == 0 || paddr < addr_start {
                addr_start = paddr;
            }
            if paddr + size > addr_end {
                addr_end = paddr + size;
            }
        }

        let ram_size = 1024 * 1024 * 1024;
        let linux_mem_idx = self.add_mem_region(ram_size - addr_start as usize, addr_start)?;

        for ph in vmlinux.elf_program_headers() {
            if ph.p_type(endian) != elf::PT_LOAD {
                continue;
            }
            let elf_start_ofs = ph.p_offset(endian) as usize;
            let elf_end_ofs = elf_start_ofs + ph.p_filesz(endian) as usize;
            let linux_mem_ofs = ph.p_paddr(endian) - addr_start;
            self.load_data_to_memory(linux_mem_idx, vmlinux_data[elf_start_ofs..elf_end_ofs].to_vec(), linux_mem_ofs);
        }

        let mut cmdline = String::from("console=ttyS0,115200");
        let mut modlist_entries = vec![];

        let hvm_mem_idx = self.add_mem_region(0x100000, 0x0)?;
        let hvm_base: u64 = 0x10000;
        let hvm_reserved_size: u64 = 0x30000;

        match &args.initramfs {
            Some(initramfs_path) => {
                let initrd_cmdline = "\0";
                let initrd_cmdline_start = (addr_end + 0xfff) & !0xfff;
                self.load_data_to_memory(linux_mem_idx, initrd_cmdline.as_bytes().to_vec(), initrd_cmdline_start);

                let initrd_start = ((initrd_cmdline.len() as u64) + initrd_cmdline_start + 0xfff) & !0xfff;
                let initramfs_size = self.load_file_to_memory(linux_mem_idx, &initramfs_path, initrd_start - addr_start).unwrap();
                modlist_entries.push(HvmModlistEntry {
                    paddr: initrd_start as u64,
                    size: initramfs_size as u64,
                    cmdline_paddr: initrd_cmdline_start as u64,
                    reserved: 0u64
                });
            },
            None => ()
        }

        let memmap_entries = [
            HvmMemmapEntry { addr: 0x0, size: hvm_base, type_: 1, reserved: 0 },
            HvmMemmapEntry { addr: hvm_base + hvm_reserved_size, size: 0x5fc00, type_: 1, reserved: 0 },
            HvmMemmapEntry { addr: 0x9fc00, size: 0x60400, type_: 2, reserved: 0 },
            HvmMemmapEntry { addr: addr_start, size: (ram_size as u64) - addr_start, type_: 1, reserved: 0}
        ];

        let mut cmdline_start = hvm_base + std::mem::size_of::<HvmStartInfo>() as u64;
        cmdline_start = (cmdline_start + 0xfff) & !0xfff;
        let cmdline_size = cmdline.len() as u64;
        self.load_data_to_memory(hvm_mem_idx, cmdline.as_bytes().to_vec(), cmdline_start);

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
        self.load_data_to_memory(hvm_mem_idx, memmap_buf, memmap_start);

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
        self.load_data_to_memory(hvm_mem_idx, modlist_buf, modlist_start);

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
        self.load_data_to_memory(hvm_mem_idx, hvm_mem_src.to_vec(), hvm_base);

        let mut regs = vcpu.get_regs()?;
        regs.rip = pvh_entrypoint;
        regs.rbx = hvm_base;
        regs.rflags = 0x2;
        vcpu.set_regs(regs);
        regs.print();

        let mut sregs2 = vcpu.get_sregs2()?;
        let code = kvm_segment {
            base: 0, limit: 0xffff_ffff, selector: 1 << 3,
            type_: 0b1010,
            present: 1, dpl: 0, db: 1, s: 1, l: 0, g: 1,
            avl: 0, unusable: 0, padding: 0,
        };
        let data = kvm_segment { selector: 2 << 3, type_: 0b0010, ..code };

        sregs2.cs = code;
        sregs2.ds = data;
        sregs2.es = data;
        sregs2.fs = data;
        sregs2.gs = data;
        sregs2.ss = data;

        sregs2.tr = kvm_segment {
            base: 0, limit: 0x67, selector: 3 << 3,
            type_: 11, s: 0, present: 1, dpl: 0, db: 0, l: 0, g: 0,
            avl: 0, unusable: 0, padding: 0,
        };
        sregs2.ldt = kvm_segment { unusable: 1, ..sregs2.tr };

        sregs2.cr0 = 0x1;
        sregs2.cr4 = 0x0;
        vcpu.set_sregs2(sregs2);


        Ok(())
    }

    pub fn arch_load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
        self.linux_pvh_boot(vcpu, args)
    }
}
