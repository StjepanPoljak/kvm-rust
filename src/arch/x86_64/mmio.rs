use std::io::{self};
use crate::VM;
use crate::MMIO;

impl VM {
     pub fn arch_init_mmio_devices(&mut self) -> io::Result<()> {
        Ok(())
    }

    pub fn arch_handle_mmio(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        if (mmio.phys_addr >> 12 == 0xb8) && (mmio.phys_addr % 2 == 0) && (mmio.is_write != 0) {
            print!("{}", mmio.data[0] as char);
        }

        Ok(())
    }
}
