include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

use std::io::{self, Read};
use std::collections::HashMap;
use std::sync::LazyLock;
use libc::{_IOW, _IO, _IOR};

use crate::KVMIO;
use crate::VCPU;

const KVM_GET_ONE_REG : u64 = _IOW::<kvm_one_reg>(KVMIO, 0xab);
const KVM_SET_ONE_REG : u64 = _IOW::<kvm_one_reg>(KVMIO, 0xac);

const KVM_REG_ARM64 : u64 = 0x6000000000000000;
const KVM_REG_SIZE_U64 : u64 = 0x0030000000000000;
const KVM_REG_ARM_COPROC_SHIFT : u64 = 16;
const KVM_REG_ARM_CORE : u64 = 0x0010 << KVM_REG_ARM_COPROC_SHIFT;

fn AARCH64_CORE_REG(name: &str) -> io::Result<u64> {
    let base = KVM_REG_ARM64 | KVM_REG_SIZE_U64 | KVM_REG_ARM_CORE;
    if name.starts_with("x") {
        let reg : u64 = name[1..]
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        return Ok(base | reg * 2); }

    match name {
        "sp" => { return Ok(base | (31 * 2)); },
        "pc" => { return Ok(base | (32 * 2)); },
        "pstate" => { return Ok(base | (33 * 2)); },
        _ => ()
    };

    Err(io::Error::other("Invalid register."))
}

fn load_regs_hash() -> io::Result<HashMap<String, u64>> {
    let mut res = HashMap::<String, u64>::new();
    let mut regs = ((0..30).map(|x| format!("x{x}")).collect::<Vec<String>>());
    regs.extend(["sp", "pc", "pstate"].iter().map(|x| x.to_string()).collect::<Vec<String>>());
    for each in regs {
        let id = AARCH64_CORE_REG(&each)?;
        res.insert(each, id);
    }
    Ok(res)
}

static REGS: LazyLock<HashMap<String, u64>> = LazyLock::new(|| {
    load_regs_hash().expect("Failed to load register hash.")
});

impl VCPU {

    pub fn get_one_reg(&mut self, name: &str) -> io::Result<u64> {
        let mut val : u64 = 0;
        let mut reg : kvm_one_reg = unsafe { std::mem::zeroed() };

        reg.id = *REGS.get(name).ok_or(io::Error::other("Invalid register name."))?;
        reg.addr = &mut val as *mut u64 as u64;

        let ret = unsafe {
            libc::ioctl(self.fd, KVM_GET_ONE_REG, &mut reg)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(val)
    }

    pub fn set_one_reg(&mut self, name: &str, val: u64) -> io::Result<()> {
        let mut reg : kvm_one_reg = unsafe { std::mem::zeroed() };

        reg.id = *REGS.get(name).ok_or(io::Error::other("Invalid register name."))?;
        reg.addr = &val as *const u64 as u64;

        let ret = unsafe {
            libc::ioctl(self.fd, KVM_SET_ONE_REG, &mut reg)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn set_ip(&mut self, ip: usize) -> io::Result<()> {
        self.set_one_reg("pc", ip as u64)
    }

    pub fn get_ip(&mut self) -> io::Result<usize> {
        let pc = self.get_one_reg("x0")?;
        Ok(pc as usize)
    }

    pub fn print_regs(&mut self) -> io::Result<()> {
        let mut col = 1;
        print!("{:>8} {:>20}", "reg", "id");
        println!("{:>8} {:>20}", "reg", "id");
        for (key, val) in REGS.iter() {
            print!("{:>8} {:>#20x}", key, val);
            if col == 1 { col = 2; }
            else if col == 2 { println!(); col = 1; }
        }
        println!();
        Ok(())
    }
}
