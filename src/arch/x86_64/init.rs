use crate::VCPU;
use crate::VM;
use crate::Args;

use std::io::{self, Read};

pub fn arch_init(vm: &mut VM, vcpu: &mut VCPU) -> io::Result<()> {
    let mut sregs2 = vcpu.get_sregs2()?;

    sregs2.cs.base = 0;
    sregs2.cs.selector = 0;

    vcpu.set_sregs2(sregs2)
}

impl VM {
    pub fn arch_load_linux(&mut self, vcpu: &mut VCPU, args: &Args) -> io::Result<()> {
	Err(io::Error::other("Loading Linux not implemented for x86."))
    }
}
