use std::io::{ self };
use std::sync::Arc;
use std::time::{ Duration, Instant };
use crate::{ IO, IODevice, VmControl, KVM_EXIT_IO_IN, KVM_EXIT_IO_OUT };
use crate::{ read_le32, write_le32 };

/* Ports for guests without an OS of their own (see guests/doom). All
 * accesses are 32 bits wide; reads of anything else return all ones. */
pub const PV_PORT_FIRST: u16 = 0x600;
pub const PV_PORT_LAST: u16 = 0x60f;
/* in: milliseconds since the VM started, wrapping at 2^32 */
const PV_CLOCK_MS: u16 = 0x600;
/* out: block the calling vcpu for that many milliseconds */
const PV_SLEEP_MS: u16 = 0x604;
/* out: stop the VM; the value becomes the VMM's exit code */
const PV_POWER_OFF: u16 = 0x608;

/* Keeps VmControl::stop() responsive while a vcpu sleeps. */
const PV_SLEEP_MAX_MS: u32 = 1000;

pub struct Paravirt {
    start: Instant,
    control: Arc<VmControl>
}

impl Paravirt {
    pub fn new(control: Arc<VmControl>) -> Self {
        Self { start: Instant::now(), control }
    }
}

impl IODevice for Paravirt {
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        let size = io.size as usize;
        let data = unsafe {
            std::slice::from_raw_parts_mut(base.add(io.data_offset as usize), size * io.count as usize)
        };
        let direction = io.direction as u32;
        if size != 4 {
            if direction == KVM_EXIT_IO_IN {
                data.fill(0xff);
            }
            return Ok(());
        }

        match (direction, io.port) {
            (KVM_EXIT_IO_IN, PV_CLOCK_MS) => write_le32(data, 0, self.start.elapsed().as_millis() as u32),
            (KVM_EXIT_IO_OUT, PV_SLEEP_MS) => {
                let ms = read_le32(data, 0).min(PV_SLEEP_MAX_MS);
                std::thread::sleep(Duration::from_millis(ms as u64));
            },
            (KVM_EXIT_IO_OUT, PV_POWER_OFF) => {
                let code = read_le32(data, 0) as i32;
                println!("Guest powered off (exit code {}).\r", code);
                self.control.power_off(code);
            },
            (KVM_EXIT_IO_IN, _) => data.fill(0xff),
            (_, _) => ()
        }
        Ok(())
    }
}
