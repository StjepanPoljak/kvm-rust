use crate::VCPU;
use crate::VM;
use crate::read_le32;
use crate::read_le64;
use crate::Args;

use std::fs::{File};
use std::io::{self, Read};

pub fn arch_init(vm: &mut VM, vcpu: &mut VCPU) -> io::Result<()> {
    vcpu.arm_vcpu_init()
}

#[repr(C)]
struct Arm64ImageHeader {
    code0: u32,
    code1: u32,
    text_offset: u64,   // le
    image_size: u64,    // le
    flags: u64,
    res2: u64,
    res3: u64,
    res4: u64,
    magic: u32,         // 0x644d5241 == "ARM\x64"
    res5: u32,
}

fn get_linux_header(path: &str) -> io::Result<Arm64ImageHeader> {
    let mut file = File::open(path)?;
    let mut buf = [0u8; 64];
    file.read_exact(&mut buf)?;
    Ok(Arm64ImageHeader{
        code0: read_le32(&buf, 0),
        code1: read_le32(&buf, 4),
        text_offset: read_le64(&buf, 8),
        image_size: read_le64(&buf, 16),
        flags: read_le64(&buf, 24),
        res2: read_le64(&buf, 32),
        res3: read_le64(&buf, 40),
        res4: read_le64(&buf, 48),
        magic: read_le32(&buf, 56),
        res5: read_le32(&buf, 60)
    })
}

const LOAD_ADDR: u64 = 0x40_000_000;
const KERN_OFFS: u64 = 0x200_000;

impl VM {
    pub fn arch_load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
        let header = get_linux_header(&args.binary)?;
        let file_len = std::fs::metadata("Image")?.len() as usize;

        println!("magic {:#x}, image_size: {}, file_size: {}, text_offset: {:#x}", header.magic, header.image_size, file_len, header.text_offset);
        let linux_mem_idx = self.add_mem_region(1024 * 1024 * 1024, LOAD_ADDR)?;

        match &args.dtb {
            Some(dtb_path) => { self.load_file_to_memory(linux_mem_idx, &dtb_path, 0x0)?; },
            None => { return Err(io::Error::other("Could not find device tree blob.")); }
        }

        self.load_file_to_memory(linux_mem_idx, &args.binary, KERN_OFFS)?;

        vcpu.set_one_reg("x0", LOAD_ADDR)?;
        vcpu.set_one_reg("x1", 0x0)?;
        vcpu.set_one_reg("x2", 0x0)?;
        vcpu.set_one_reg("x3", 0x0)?;

        vcpu.set_one_reg("pc", LOAD_ADDR + KERN_OFFS)?;

        vcpu.print_regs()
    }
}
