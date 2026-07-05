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
