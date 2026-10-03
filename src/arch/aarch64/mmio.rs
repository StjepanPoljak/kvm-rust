use std::io::self;
use crate::{ VM, MMIO, MMIODevice, KVMIO, MAIN_TID, TTY, start_tty };
use crate::{ write_le32, read_le32 };
use std::collections::HashMap;
use libc::_IOW;
use std::time::Duration;
use std::thread;
use std::sync::{ Arc, Mutex, LazyLock };
use std::io::{ stdin, stdout, Read, Write };
use termion::raw::IntoRawMode;
use std::sync::atomic::{ Ordering, AtomicU64 };

include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

const KVM_IRQ_LINE: libc::Ioctl = _IOW::<kvm_irq_level>(KVMIO, 0x61);

pub struct UART {
    rx: Option<u8>,
    fr: u32, // 0x18
    cr: u32,   // 0x30
    imsc: u32,   // 0x38
    lcr: u32,   // 0x2c
    ibrd: u32,   // 0x24
    fbrd: u32,   // 0x28
    ris: u32,    // 0x3c
    icr: u32, // 0x44
    vm_fd: libc::c_int
}

impl TTY for UART {
    fn send_char(&mut self, ch: u8) -> io::Result<()> {
        self.rx = Some(ch);
        self.ris |= 1 << 4;
        self.update_irq()
    }
}

pub fn arch_update_irq(level: u32, vm_fd: libc::c_int) -> io::Result<()> {
    let mut irq : kvm_irq_level = unsafe { std::mem::zeroed() };
    irq.level = level;
    irq.__bindgen_anon_1.irq = 0x1000021;

    let mut ret = unsafe {
        libc::ioctl(vm_fd, KVM_IRQ_LINE, &mut irq)
    };

    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

impl MMIODevice for UART {

    fn handle(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        let mmio_reg = mmio.phys_addr & 0xfff;
        if (mmio.is_write != 0) && mmio_reg == 0x0 {
            /* we take only first u8 element of data */
            print!("{}", mmio.data[0] as char);
            io::stdout().flush()?;
        } else if mmio.is_write == 0 {
            let resp = Vec::<u8>::new();
            write_le32(&mut mmio.data, 0, self.pl011_response((mmio_reg) as u32)?);
        } else if mmio.is_write != 0 {
            let val = read_le32(&mmio.data, 0);
            match mmio_reg {
                0x24 => { self.ibrd = val; },
                0x28 => { self.fbrd = val; },
                0x2c => { self.lcr = val; },
                0x30 => { self.cr = val; },
                0x38 => { self.imsc = val;
                          self.update_irq()?; }
                0x44 => { self.ris &= !val;
                          self.update_irq()?; }
                _ => ()
            }
        }

        Ok(())
    }
}

fn load_pl011_id_map() -> HashMap<u32, u32> {
    HashMap::from([
        (0xfe0, 0x11), (0xfe4, 0x10), (0xfe8, 0x14), (0xfec, 0x00), // PeriphID0-3
        (0xff0, 0x0d), (0xff4, 0xf0), (0xff8, 0x05), (0xffc, 0xb1) // PCellID0-3
    ])
}

static PL011_ID_MAP: LazyLock<HashMap<u32, u32>> = LazyLock::new(|| {
    load_pl011_id_map()
});

impl UART {
    fn new(vm_fd: libc::c_int) -> io::Result<Arc<Mutex<Self>>> {
        let uart = Arc::new(Mutex::new(UART {
            fr: 0x90, cr: 0x0, imsc: 0x0, lcr: 0x0, ibrd: 0x0, fbrd: 0x0, icr: 0x0, ris: 0x0, rx: None, vm_fd
        }));

        start_tty(uart.clone());
        Ok(uart)
    }

    fn pl011_response(&mut self, inb: u32) -> io::Result<u32> {
        if let Some(ret) = PL011_ID_MAP.get(&inb) {
            return Ok(*ret);
        }

        match inb {
            0x0 => { let ch = self.rx.take().unwrap_or(0);
                     self.ris &= !(1 << 4);
                     self.update_irq()?;
                     return Ok(ch as u32);
            },
            0x18 => Ok(0x80 | if self.rx.is_none() { 0x10 } else { 0 } ),
            0x24 => Ok(self.ibrd),
            0x28 => Ok(self.fbrd),
            0x2c => Ok(self.lcr),
            0x30 => Ok(self.cr),
            0x3c => Ok(self.ris),
            0x38 => Ok(self.imsc),
            0x40 => Ok(self.ris & self.imsc),
            0x44 => Ok(self.icr),
            _ => Ok(0x0)
        }
    }

    pub fn update_irq(&mut self) -> io::Result<()> {
        arch_update_irq(((self.ris & self.imsc) != 0) as u32, self.vm_fd)
    }
}

impl VM {
    pub fn arch_init_mmio_devices(&mut self) -> io::Result<()> {
        let uart = UART::new(self.fd)?;
        self.mmio_devices.insert(0x9000, uart.clone() as Arc<Mutex<dyn MMIODevice>>);
        Ok(())
    }

    pub fn arch_handle_mmio(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        let page = mmio.phys_addr >> 12;
        if let Some(device) = self.mmio_devices.get_mut(&page) {
            device.lock().unwrap().handle(mmio)?;
        }

        Ok(())
    }
}

