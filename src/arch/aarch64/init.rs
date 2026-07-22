include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

use libc::{_IOWR,_IOW};
use crate::VCPU;
use crate::VM;
use crate::read_le32;
use crate::read_le64;
use crate::Args;
use crate::mmap_mem_region;

use std::fs::{File};
use std::io::{self, Read};

use crate::KVMIO;

const KVM_ARM_DEVICE_VGIC_V2 : u32 = 0x5;
const KVM_VGIC_V2_ADDR_TYPE_DIST : u64 = 0x0;
const KVM_VGIC_V2_DIST_SIZE : usize = 0x1000;
const KVM_VGIC_V2_CPU_SIZE : usize = 0x2000;
const KVM_VGIC_V2_ADDR_TYPE_CPU : u64 = 0x1;
const KVM_DEV_ARM_VGIC_GRP_ADDR : u32 = 0x0;

const KVM_CREATE_DEVICE : u64 = _IOWR::<kvm_create_device>(KVMIO, 0xe0);
const KVM_SET_DEVICE_ATTR : u64 = _IOW::<kvm_device_attr>(KVMIO, 0xe1);

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
const INITRAMFS_OFFS: u64 = 0x200_000;
const KERN_OFFS: u64 = 0x200_000 + INITRAMFS_OFFS;

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

	match &args.initramfs {
	    Some(initramfs_path) => { self.load_file_to_memory(linux_mem_idx, &initramfs_path, INITRAMFS_OFFS)?; },
	    None => ()
	}

        self.load_file_to_memory(linux_mem_idx, &args.binary, KERN_OFFS)?;

        vcpu.set_one_reg("x0", LOAD_ADDR)?;
        vcpu.set_one_reg("x1", 0x0)?;
        vcpu.set_one_reg("x2", 0x0)?;
        vcpu.set_one_reg("x3", 0x0)?;

        vcpu.set_one_reg("pc", LOAD_ADDR + KERN_OFFS)?;

        vcpu.print_regs()?;

        let mut create_device : kvm_create_device = unsafe { std::mem::zeroed() };

        create_device.type_ = KVM_ARM_DEVICE_VGIC_V2;
	create_device.fd = 0xffffffff;
	create_device.flags = 0;

        let mut ret = unsafe {
            libc::ioctl(self.fd, KVM_CREATE_DEVICE, &mut create_device)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

	let dist_addr : u64 = 0x08_000_000;
	let cpu_addr: u64 = 0x08_010_000;
        let mut dev_attr : kvm_device_attr = unsafe { std::mem::zeroed() };
	dev_attr.group = KVM_DEV_ARM_VGIC_GRP_ADDR;
	dev_attr.attr = KVM_VGIC_V2_ADDR_TYPE_DIST;
	dev_attr.flags = 0;
	dev_attr.addr = &dist_addr as *const u64 as u64;

	ret = unsafe {
            libc::ioctl(create_device.fd as i32, KVM_SET_DEVICE_ATTR, &mut dev_attr)
	};
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }


        let mut dev_attr2 : kvm_device_attr = unsafe { std::mem::zeroed() };

        dev_attr2 = unsafe { std::mem::zeroed() };
	dev_attr2.group = KVM_DEV_ARM_VGIC_GRP_ADDR;
	dev_attr2.attr = KVM_VGIC_V2_ADDR_TYPE_CPU;
	dev_attr2.flags = 0;
	dev_attr2.addr = &cpu_addr as *const u64 as u64;

	ret = unsafe {
            libc::ioctl(create_device.fd as i32, KVM_SET_DEVICE_ATTR, &mut dev_attr2)
	};
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

	Ok(())
    }
}
