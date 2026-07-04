use crate::VCPU;
use crate::VM;
use crate::Args;

use std::io::{self, Read};

pub fn arch_init(vm: &mut VM, vcpu: &mut VCPU, mem_region_idx: usize, args: &Args) -> io::Result<()> {
    vcpu.arm_vcpu_init()?;
    vcpu.set_one_reg("pc", args.load_addr)?;
    let pc = vcpu.get_one_reg("pc")?;
    println!("pc = {:#x}", pc);
    
    let pstate : u64 = vcpu.get_one_reg("pstate")?;
    println!("pstate = {:#x}", pstate);

    vm.load_file_to_memory(mem_region_idx, &args.binary, pc as usize)?;

    Ok(())
}
