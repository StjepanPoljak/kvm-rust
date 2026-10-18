use crate::{ VM, IO, KVMIO, kvm_irq_level, KVM_ARM_IRQ_TYPE_SPI, KVM_ARM_IRQ_TYPE_SHIFT };
use libc::_IOW;
use std::io::{self};

const KVM_IRQ_LINE: libc::Ioctl = _IOW::<kvm_irq_level>(KVMIO, 0x61);
/* SPIs start at GIC interrupt ID 32. */
const GIC_SPI_BASE: u32 = 32;

/* `num` is the SPI number, as used in the device tree interrupts property. */
pub fn arch_update_irq(level: u32, vm_fd: libc::c_int, num: u32) -> io::Result<()> {
    let mut irq : kvm_irq_level = unsafe { std::mem::zeroed() };
    irq.level = level;
    irq.__bindgen_anon_1.irq = (KVM_ARM_IRQ_TYPE_SPI << KVM_ARM_IRQ_TYPE_SHIFT) | (GIC_SPI_BASE + num);

    let ret = unsafe {
        libc::ioctl(vm_fd, KVM_IRQ_LINE, &mut irq)
    };

    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

impl VM {
    pub fn arch_handle_io(&mut self, _io: &IO, _base: *mut u8) -> io::Result<()> { Ok(()) } 

    pub fn arch_init_io_devices(&mut self) -> io::Result<()> { Ok(()) }
}
