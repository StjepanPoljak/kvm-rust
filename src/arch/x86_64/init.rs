use crate::VCPU;
use crate::VM;
use crate::Args;

use std::io::{self, Read};

pub fn arch_init(vm: &mut VM, vcpu: &mut VCPU, mem_region_idx: usize, args: &Args) -> io::Result<()> {
    let mut sregs2 = vcpu.get_sregs2()?;
    sregs2.cs.base = 0;
    sregs2.cs.selector = 0;
    sregs2.print();
    vcpu.set_sregs2(sregs2)?;

    let mut regs = vcpu.get_regs()?;
    regs.rip = 0x1000;
    regs.print();
    vcpu.set_regs(regs)?;

    vm.load_file_to_memory(mem_region_idx, &args.binary, regs.rip as usize)?;

    Ok(())
}
