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

const LOAD_ADDR: u64 = 0x20_000;
const KERN_OFFS: u64 = 0x0;
const CMDLINE_ADDR: u64 = 0x10_000;

impl VM {
    fn load_elf_and_get_rip(&mut self, path: &str) -> io::Result<u64> {
        let vmlinux_data = std::fs::read(path)?;
        let vmlinux = ElfFile64::<Endianness>::parse(&*vmlinux_data).unwrap();
        let endian = vmlinux.endian();
        let mut ret = 0x0;

        let mut notes = vmlinux.section_by_name(".notes").unwrap()
            .elf_section_header()
            .notes(endian, vmlinux.data()).unwrap()
            .unwrap();

        while let Some(note) = notes.next().unwrap() {
            if note.n_type(endian) == object::elf::NoteType(18) {
                let d = note.desc();
                ret = match d.len() {
                    4 => Ok(read_le32(d, 0) as u64),
                    8 => Ok(read_le64(d, 0)),
                    _ => Err(io::Error::other(format!("unexpected PHYS32_ENTRY descsz {}", d.len())))
                }?;
                break;
            }
        }

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

        let linux_mem_idx = self.add_mem_region(1024 * 1024 * 1024, addr_start)?;

        for ph in vmlinux.elf_program_headers() {
            if ph.p_type(endian) != elf::PT_LOAD {
                continue;
            }
            let elf_start_ofs = ph.p_offset(endian) as usize;
            let elf_end_ofs = elf_start_ofs + ph.p_filesz(endian) as usize;
            let linux_mem_ofs = ph.p_paddr(endian) - addr_start;
            println!("elf_slice=[{:#x}..{:#x}], linux_mem_ofs={:#x}", elf_start_ofs, elf_end_ofs, linux_mem_ofs);
            self.load_data_to_memory(linux_mem_idx, vmlinux_data[elf_start_ofs..elf_end_ofs].to_vec(), linux_mem_ofs);
        }

        Ok(ret)
    }

    pub fn arch_load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {

        let hvm_mem_idx = self.add_mem_region(0x10000 + 0x5fc00 + 0x60400, 0x0)?;

        let mut magic = [ 'x', 'E', 'n', '3' ] .map(|c| c as u8);
        magic[1] |= 0x80;

        let ram_size = 1024 * 1024 * 1024;

        let entries = [
            HvmMemmapEntry { addr: 0x0, size: 0x10000, type_: 1, reserved: 0 },
            HvmMemmapEntry { addr: 0x40000, size: 0x5fc00, type_: 1, reserved: 0 },
            HvmMemmapEntry { addr: 0x9fc00, size: 0x60400, type_: 2, reserved: 0 },
            HvmMemmapEntry { addr: 0x100000, size: ram_size - 0x100000, type_: 1, reserved: 0 },
        ];

        let mut buf = Vec::with_capacity(entries.len() * 24);
        for e in &entries {
            buf.extend_from_slice(&e.addr.to_le_bytes());
            buf.extend_from_slice(&e.size.to_le_bytes());
            buf.extend_from_slice(&e.type_.to_le_bytes());
            buf.extend_from_slice(&e.reserved.to_le_bytes());
        }
        self.load_data_to_memory(hvm_mem_idx, buf, 0x30000);

        let cmdline = "earlyprintk=serial,ttyS0,115200 console=ttyS0,115200";
        self.load_data_to_memory(hvm_mem_idx, cmdline.as_bytes().to_vec(), 0x20000);

        let hvm_start_info = HvmStartInfo {
            magic: read_le32(&magic, 0x0),
            version: 1u32,
            flags: 0u32,
            nr_modules: 0u32,
            modlist_paddr: 0u64,
            cmdline_paddr: 0x20000u64,
            rsdp_paddr: 0u64,
            memmap_paddr: 0x30000u64,
            memmap_entries: 4u32,
            reserved: 0u32
        };

        let src = unsafe { std::slice::from_raw_parts(
            &hvm_start_info as *const HvmStartInfo as *const u8,
            std::mem::size_of::<HvmStartInfo>(),
        ) };

        self.load_data_to_memory(hvm_mem_idx, src.to_vec(), 0x10000);

        let mut regs = vcpu.get_regs()?;

        regs.rip = self.load_elf_and_get_rip(&args.binary)?;
        regs.rbx = 0x10000;
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
}
