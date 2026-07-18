use std::io::self;
use crate::VM;
use crate::MMIODevice;
use crate::write_le32;
use crate::MMIO;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

pub struct UART {
    cr:   u32,   // 0x30
    imsc: u32,   // 0x38
    lcr:  u32,   // 0x2c
    ibrd: u32,   // 0x24
    fbrd: u32    // 0x28
}

impl MMIODevice for UART {
    fn handle(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        let mmio_reg = mmio.phys_addr & 0xfff;
        if (mmio.is_write != 0) && mmio_reg == 0x0 {
            /* we take only first u8 element of data */
            print!("{}", mmio.data[0] as char);
        } else {
            let resp = Vec::<u8>::new();
            write_le32(&mut mmio.data, 0, self.pl011_response((mmio_reg) as u32)?);
        }

        Ok(())
    }
}

impl UART {
    fn new() -> Self {
        UART {
            cr: 0x0, imsc: 0x0, lcr: 0x0, ibrd: 0x0, fbrd: 0x0
        }
    }

    fn pl011_response(&mut self, inb: u32) -> io::Result<u32> {
        let pl011_map : HashMap<u32, u32> = HashMap::from([
            (0xfe0, 0x11), (0xfe4, 0x10), (0xfe8, 0x14), (0xfec, 0x00), // PeriphID0-3
            (0xff0, 0x0d), (0xff4, 0xf0), (0xff8, 0x05), (0xffc, 0xb1), // PCellID0-3
            (0x018, 0x90)
        ]);

        if let Some(ret) = pl011_map.get(&inb) {
            return Ok(*ret);
        }

        if inb == 0x30 {
            return Ok(self.cr);
        }

        Ok(0x0)
    }
}

impl VM {
    pub fn arch_init_mmio_devices(&mut self) -> io::Result<()> {
        self.mmio_devices.insert(0x9000, Box::new(UART::new()));
        Ok(())
    }

    pub fn arch_handle_mmio(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        let page = mmio.phys_addr >> 12;
        if let Some(device) = self.mmio_devices.get_mut(&page) {
            device.handle(mmio)?;
        }
        Ok(())
    }
}
