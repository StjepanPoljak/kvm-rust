use crate::VCPU;
use crate::VM;
use crate::Args;

use std::io::{self, Read};

pub fn arch_init(vm: &mut VM, vcpu: &mut VCPU) -> io::Result<()> {
    vcpu.arm_vcpu_init()
}
