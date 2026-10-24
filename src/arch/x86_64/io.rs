use std::io::{ self, Write };
use crate::{ VM, IO, IODevice, KVMIO, TTY, VmControl, start_tty, kvm_irq_level, gpa_to_host, KVM_EXIT_IO_IN, KVM_EXIT_IO_OUT };
use std::sync::{ Arc, Mutex };
use libc::_IOW;
use std::collections::VecDeque;
use super::i8042::{ I8042, I8042_DATA, I8042_COMMAND };
use super::vga::{ Vga, VGA_PORTS, VGA_FRAMEBUFFER, VGA_FRAMEBUFFER_SIZE };

const KVM_IRQ_LINE: libc::Ioctl = _IOW::<kvm_irq_level>(KVMIO, 0x61);

pub fn arch_update_irq(level: u32, vm_fd: libc::c_int, num: u32) -> io::Result<()> {
    let mut irq : kvm_irq_level = unsafe { std::mem::zeroed() };
    irq.level = level;
    irq.__bindgen_anon_1.irq = num;

    let ret = unsafe {
        libc::ioctl(vm_fd, KVM_IRQ_LINE, &mut irq)
    };

    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

pub struct UART {
    ier: u8, /* 0x3f9 */
    fcr: u8, /* 0x3fa */
    lcr: u8, /* 0x3fb */
    mcr: u8, /* 0x3fc */
    #[allow(dead_code)]
    lsr: u8, /* 0x3fd */
    #[allow(dead_code)]
    msr: u8, /* 0x3fe */
    scr: u8, /* 0x3ff */
    thr_ipending: bool,
    irq_level: bool,
    vm_fd: libc::c_int,
    rx: VecDeque<u8>
}

impl TTY for UART {
    fn send_char(&mut self, ch: u8) -> io::Result<()> {
        self.rx.push_back(ch);
        self.update_irq()
    }
}

impl UART {
    fn update_irq(&mut self) -> io::Result<()> {
        let level = self.mcr & 0x08 != 0
            && ((self.ier & 0x01 != 0 && !self.rx.is_empty())
            || (self.ier & 0x02 != 0 && self.thr_ipending));

        if level != self.irq_level {
            self.irq_level = level;
            arch_update_irq(if level { 1 } else { 0 } as u32, self.vm_fd, 0x4)?;
        }

        Ok(())
    }

    fn new(vm_fd: libc::c_int, control: Arc<VmControl>) -> io::Result<Arc<Mutex<Self>>> {
        let uart = Arc::new(Mutex::new(UART { ier: 0x0, fcr: 0x0, lcr: 0x0, mcr: 0x0, lsr: 0x0, msr: 0x0, scr: 0x0, thr_ipending: false, irq_level: false, vm_fd, rx: VecDeque::new() }));

        start_tty(uart.clone(), control);
        Ok(uart)
    }
}

impl IODevice for UART {
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        let port = io.port;
        let direction = io.direction as u32;
        let size = io.size as usize;
        let count = io.count as usize;
        let offset = io.data_offset as usize;
            let data = unsafe {
                std::slice::from_raw_parts_mut(base.add(offset), size * count)
        };
        match (direction, port) {
            (KVM_EXIT_IO_OUT, 0x3f8) => {
                if data[0] == b'\n' {
                    io::stdout().write_all(b"\r").ok();
                }
                io::stdout().write_all(&data[0..1]).ok();
                io::stdout().flush().ok();
                self.thr_ipending = true;
            },
            (KVM_EXIT_IO_IN, 0x3f8) => data[0] = self.rx.pop_front().unwrap_or(0),
            (KVM_EXIT_IO_IN, 0x3fd) => data[0] = if self.rx.is_empty() { 0x60 } else { 0x61 },
            (KVM_EXIT_IO_OUT, 0x3f9) => {
                let v = data[0] & 0x0f;
                if v & 0x02 != 0 && self.ier & 0x02 == 0 {
                    self.thr_ipending = true;
                }
                self.ier = v;
            },
            (KVM_EXIT_IO_OUT, 0x3fa) => self.fcr = data[0] as u8,
            (KVM_EXIT_IO_OUT, 0x3fb) => self.lcr = data[0] as u8,
            (KVM_EXIT_IO_OUT, 0x3fc) => self.mcr = data[0] as u8,
            (KVM_EXIT_IO_IN, 0x3f9) => data[0] = self.ier,
            (KVM_EXIT_IO_IN, 0x3fa) => {
                let fifo = if self.fcr & 0x01 != 0 { 0xc0 } else { 0x00 };
                data[0] = if self.ier & 0x01 != 0 && !self.rx.is_empty() {
                    fifo | 0x04
                } else if self.ier & 0x02 != 0 && self.thr_ipending {
                    self.thr_ipending = false;
                    fifo | 0x02
                } else {
                    fifo | 0x01
                };
            },
            (KVM_EXIT_IO_IN, 0x3fe) => data[0] = if self.mcr & 0x10 != 0 {
                    ((self.mcr & 0x02) << 3) | ((self.mcr & 0x01) << 5)
                  | ((self.mcr & 0x04) << 4) | ((self.mcr & 0x08) << 4)
                } else { 0xb0 },
            (KVM_EXIT_IO_IN, 0x3ff) => data[0] = self.scr,
            (KVM_EXIT_IO_OUT, 0x3ff) => self.scr = data[0],
            (KVM_EXIT_IO_IN, 0x3f8..=0x3ff) => data[0] = 0x00,
            (KVM_EXIT_IO_OUT, _) => {},
            (KVM_EXIT_IO_IN, _) => data[0] = 0xff,
            (_, _) => data[0] = 0xff
        };

        self.update_irq()
    }
}

#[repr(u64)]
pub enum DeviceId {
    Uart = 0u64,
    I8042 = 1u64,
    Vga = 2u64,
    Unknown = !(0u64)
}

impl VM {
    pub fn arch_init_io_devices(&mut self) -> io::Result<()> {
        let uart = UART::new(self.fd, self.control.clone())?;
        self.io_devices.insert(DeviceId::Uart as u64, uart.clone() as Arc<Mutex<dyn IODevice>>);
        let i8042 = I8042::new(self.fd, self.control.clone());
        self.io_devices.insert(DeviceId::I8042 as u64, i8042.clone() as Arc<Mutex<dyn IODevice>>);
        self.keyboard = Some(i8042);
        let framebuffer = gpa_to_host(&self.guest_memory, VGA_FRAMEBUFFER, VGA_FRAMEBUFFER_SIZE);
        let vga = Vga::new(framebuffer.map(|p| p as *const u8));
        self.io_devices.insert(DeviceId::Vga as u64, vga.clone() as Arc<Mutex<dyn IODevice>>);
        self.display = Some(vga);
        Ok(())
    }

    pub fn arch_handle_io(&self, io: &IO, base: *mut u8) -> io::Result<()> {
        let dev: DeviceId = match io.port {
            0x3f8..=0x3ff => DeviceId::Uart,
            I8042_DATA | I8042_COMMAND => DeviceId::I8042,
            port if VGA_PORTS.contains(&port) => DeviceId::Vga,
            _ => DeviceId::Unknown
        };
        if let Some(device) = self.io_devices.get(&(dev as u64)) {
            device.lock().unwrap().handle(io, base)?;
        }

        Ok(())
    }
}

