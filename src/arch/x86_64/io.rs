use std::io::{ self, Write };
use crate::{ VM, IO, IODevice, MAIN_TID };
use std::sync::{ Arc, Mutex };

const KVM_EXIT_IO_OUT: u8 = 1;
const KVM_EXIT_IO_IN: u8 = 0;

pub struct UART {

}

impl UART {
    fn new(vm_fd: libc::c_int) -> io::Result<Arc<Mutex<Self>>> {
        let uart = Arc::new(Mutex::new(UART {}));
        Ok(uart)
    }
}

impl IODevice for UART {
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        let port = io.port;
        let direction = io.direction;
        let size = io.size as usize;
        let count = io.count as usize;
        let offset = io.data_offset as usize;
        let data = unsafe {
            std::slice::from_raw_parts_mut(base.add(offset), size * count)
        };
        match (direction, port) {
            (KVM_EXIT_IO_OUT, 0x3f8) => {
                print!("{}", data[0] as char);
                io::stdout().flush().ok();
            }
            (KVM_EXIT_IO_IN, 0x3fd) => data[0] = 0x60,   // LSR: THRE | TEMT
            (KVM_EXIT_IO_IN, 0x3f8..=0x3ff) => data[0] = 0x00,
            (KVM_EXIT_IO_OUT, _) => {},
            (KVM_EXIT_IO_IN, _) => data[0] = 0xff,
            (_, _) => data[0] = 0xff
        };
        Ok(())
    }
}

impl VM {
    pub fn arch_init_io_devices(&mut self) -> io::Result<()> {
        let uart = UART::new(self.fd)?;
        self.io_devices.insert(0x3f, uart.clone() as Arc<Mutex<dyn IODevice>>);
        Ok(())
    }

    pub fn arch_handle_io(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        let dev = (io.port as u64) >> 4;
        if let Some(device) = self.io_devices.get_mut(&dev) {
            device.lock().unwrap().handle(io, base)?;
        }
        Ok(())
    }
}

