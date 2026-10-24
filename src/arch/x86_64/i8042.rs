use std::io::{ self };
use std::collections::VecDeque;
use std::sync::{ Arc, Mutex };
use crate::{ IO, IODevice, Keyboard, VmControl, KVM_EXIT_IO_IN, KVM_EXIT_IO_OUT };
use super::io::arch_update_irq;

/* The PS/2 controller (8042) with a keyboard on its first port and
 * nothing on the second. The keyboard sends scancode set 1, what a guest
 * reads by default with the controller's translation on. Keys come from
 * the frontends as X11 keysyms (Keyboard::key_event). */

pub const I8042_DATA: u16 = 0x60;
pub const I8042_COMMAND: u16 = 0x64;
const I8042_IRQ: u32 = 1;

/* Status register (read 0x64) */
const STATUS_OBF: u8 = 0x01;       /* output buffer full */
const STATUS_SYS: u8 = 0x04;       /* passed self-test */
const STATUS_CMD: u8 = 0x08;       /* last write was to 0x64 */
const STATUS_UNLOCKED: u8 = 0x10;

/* Controller configuration byte */
const CONFIG_KBD_INT: u8 = 0x01;
const CONFIG_SYS: u8 = 0x04;
const CONFIG_KBD_DISABLE: u8 = 0x10;
const CONFIG_AUX_DISABLE: u8 = 0x20;
const CONFIG_TRANSLATE: u8 = 0x40;

/* Controller commands (write 0x64) */
const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_AUX_DISABLE: u8 = 0xa7;
const CMD_AUX_ENABLE: u8 = 0xa8;
const CMD_SELF_TEST: u8 = 0xaa;
const CMD_KBD_TEST: u8 = 0xab;
const CMD_KBD_DISABLE: u8 = 0xad;
const CMD_KBD_ENABLE: u8 = 0xae;
const CMD_WRITE_OUTPUT_PORT: u8 = 0xd1;
const CMD_WRITE_KBD_OUTPUT: u8 = 0xd2;
const CMD_PULSE_RESET: u8 = 0xfe;

/* Keyboard commands (write 0x60) and replies */
const KBD_SET_LEDS: u8 = 0xed;
const KBD_ECHO: u8 = 0xee;
const KBD_SCANCODE_SET: u8 = 0xf0;
const KBD_GET_ID: u8 = 0xf2;
const KBD_SET_TYPEMATIC: u8 = 0xf3;
const KBD_ENABLE: u8 = 0xf4;
const KBD_DISABLE: u8 = 0xf5;
const KBD_RESET: u8 = 0xff;
const KBD_ACK: u8 = 0xfa;
const KBD_SELF_TEST_OK: u8 = 0xaa;
/* An MF2 keyboard */
const KBD_ID: [u8; 2] = [0xab, 0x83];

pub struct I8042 {
    config: u8,
    /* What the guest reads from 0x60 next; queue holds the rest. */
    output: Option<u8>,
    queue: VecDeque<u8>,
    /* A controller command waiting for its byte on 0x60 */
    controller_param: Option<u8>,
    /* A keyboard command waiting for its parameter byte */
    keyboard_param: Option<u8>,
    status_cmd: bool,
    scanning: bool,
    irq_level: bool,
    vm_fd: libc::c_int,
    control: Arc<VmControl>
}

impl I8042 {
    pub fn new(vm_fd: libc::c_int, control: Arc<VmControl>) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            config: CONFIG_KBD_INT | CONFIG_SYS | CONFIG_AUX_DISABLE | CONFIG_TRANSLATE,
            output: None,
            queue: VecDeque::new(),
            controller_param: None,
            keyboard_param: None,
            status_cmd: false,
            scanning: true,
            irq_level: false,
            vm_fd,
            control
        }))
    }

    fn push(&mut self, byte: u8) {
        self.queue.push_back(byte);
        if self.output.is_none() {
            self.output = self.queue.pop_front();
        }
    }

    /* Edge-triggered on the IOAPIC: the line has to drop between bytes,
     * which reading 0x60 does before the next byte is loaded. */
    fn update_irq(&mut self) -> io::Result<()> {
        let level = self.output.is_some() && self.config & CONFIG_KBD_INT != 0;
        if level != self.irq_level {
            self.irq_level = level;
            arch_update_irq(level as u32, self.vm_fd, I8042_IRQ)?;
        }
        Ok(())
    }

    fn read_data(&mut self) -> io::Result<u8> {
        let byte = self.output.take().unwrap_or(0);
        self.update_irq()?;
        self.output = self.queue.pop_front();
        Ok(byte)
    }

    fn status(&self) -> u8 {
        let mut status = STATUS_UNLOCKED;
        if self.output.is_some() {
            status |= STATUS_OBF;
        }
        if self.config & CONFIG_SYS != 0 {
            status |= STATUS_SYS;
        }
        if self.status_cmd {
            status |= STATUS_CMD;
        }
        status
    }

    fn controller_command(&mut self, cmd: u8) {
        match cmd {
            CMD_READ_CONFIG => self.push(self.config),
            CMD_WRITE_CONFIG | CMD_WRITE_OUTPUT_PORT | CMD_WRITE_KBD_OUTPUT => self.controller_param = Some(cmd),
            CMD_AUX_DISABLE => self.config |= CONFIG_AUX_DISABLE,
            CMD_AUX_ENABLE => self.config &= !CONFIG_AUX_DISABLE,
            CMD_SELF_TEST => {
                self.config |= CONFIG_SYS;
                self.push(0x55);
            },
            CMD_KBD_TEST => self.push(0x00),
            CMD_KBD_DISABLE => self.config |= CONFIG_KBD_DISABLE,
            CMD_KBD_ENABLE => self.config &= !CONFIG_KBD_DISABLE,
            CMD_PULSE_RESET => {
                /* The classic PC reboot. There is no reset to go through, so stop. */
                println!("Guest reset through the i8042.\r");
                self.control.stop();
            },
            /* Commands for the absent second port go unanswered, which
             * is how a guest tells that nothing is there. */
            _ => ()
        }
    }

    fn controller_param(&mut self, cmd: u8, byte: u8) {
        match cmd {
            CMD_WRITE_CONFIG => self.config = byte,
            /* Bit 0 low resets the CPU. */
            CMD_WRITE_OUTPUT_PORT if byte & 1 == 0 => self.controller_command(CMD_PULSE_RESET),
            CMD_WRITE_KBD_OUTPUT => self.push(byte),
            _ => ()
        }
    }

    fn keyboard_command(&mut self, byte: u8) {
        /* Writing to the keyboard enables its port, as on real hardware. */
        self.config &= !CONFIG_KBD_DISABLE;

        if let Some(cmd) = self.keyboard_param.take() {
            match cmd {
                KBD_SCANCODE_SET if byte == 0 => {
                    self.push(KBD_ACK);
                    self.push(1);
                },
                _ => self.push(KBD_ACK)
            }
            return;
        }

        match byte {
            KBD_SET_LEDS | KBD_SCANCODE_SET | KBD_SET_TYPEMATIC => {
                self.keyboard_param = Some(byte);
                self.push(KBD_ACK);
            },
            KBD_ECHO => self.push(KBD_ECHO),
            KBD_GET_ID => {
                self.push(KBD_ACK);
                KBD_ID.iter().for_each(|b| self.push(*b));
            },
            KBD_ENABLE => {
                self.scanning = true;
                self.push(KBD_ACK);
            },
            KBD_DISABLE => {
                self.scanning = false;
                self.push(KBD_ACK);
            },
            KBD_RESET => {
                self.scanning = true;
                self.queue.clear();
                self.push(KBD_ACK);
                self.push(KBD_SELF_TEST_OK);
            },
            /* Defaults, resend and the rest: just acknowledge. */
            _ => self.push(KBD_ACK)
        }
    }
}

impl IODevice for I8042 {
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        let data = unsafe { std::slice::from_raw_parts_mut(base.add(io.data_offset as usize), io.size as usize) };
        match (io.direction as u32, io.port) {
            (KVM_EXIT_IO_IN, I8042_DATA) => data[0] = self.read_data()?,
            (KVM_EXIT_IO_IN, I8042_COMMAND) => data[0] = self.status(),
            (KVM_EXIT_IO_OUT, I8042_DATA) => {
                self.status_cmd = false;
                match self.controller_param.take() {
                    Some(cmd) => self.controller_param(cmd, data[0]),
                    None => self.keyboard_command(data[0])
                }
            },
            (KVM_EXIT_IO_OUT, I8042_COMMAND) => {
                self.status_cmd = true;
                self.controller_param = None;
                self.controller_command(data[0]);
            },
            (KVM_EXIT_IO_IN, _) => data.fill(0xff),
            (_, _) => ()
        }
        self.update_irq()
    }
}

impl Keyboard for I8042 {
    fn key_event(&mut self, down: bool, keysym: u32) -> io::Result<()> {
        if !self.scanning || self.config & CONFIG_KBD_DISABLE != 0 {
            return Ok(());
        }
        let Some((extended, code)) = keysym_to_set1(keysym) else { return Ok(()) };
        if extended {
            self.push(0xe0);
        }
        self.push(if down { code } else { code | 0x80 });
        self.update_irq()
    }
}

/* X11 keysym to set 1 make code, US layout; true means an 0xe0 prefix.
 * Shifted characters map to the key that types them. */
fn keysym_to_set1(keysym: u32) -> Option<(bool, u8)> {
    const ROW_QWERTY: &[u8] = b"qwertyuiop";
    const ROW_ASDF: &[u8] = b"asdfghjkl";
    const ROW_ZXCV: &[u8] = b"zxcvbnm";

    let code = match keysym {
        0x30 => 0x0b,                                      /* 0 */
        0x31..=0x39 => keysym as u8 - 0x31 + 0x02,         /* 1-9 */
        0x41..=0x5a | 0x61..=0x7a => {
            let c = (keysym as u8).to_ascii_lowercase();
            if let Some(i) = ROW_QWERTY.iter().position(|&k| k == c) {
                0x10 + i as u8
            } else if let Some(i) = ROW_ASDF.iter().position(|&k| k == c) {
                0x1e + i as u8
            } else {
                0x2c + ROW_ZXCV.iter().position(|&k| k == c)? as u8
            }
        },
        0x21 => 0x02, 0x40 => 0x03, 0x23 => 0x04, 0x24 => 0x05, 0x25 => 0x06,   /* ! @ # $ % */
        0x5e => 0x07, 0x26 => 0x08, 0x2a => 0x09, 0x28 => 0x0a, 0x29 => 0x0b,   /* ^ & * ( ) */
        0x2d | 0x5f => 0x0c,                               /* - _ */
        0x3d | 0x2b => 0x0d,                               /* = + */
        0x5b | 0x7b => 0x1a,                               /* [ { */
        0x5d | 0x7d => 0x1b,                               /* ] } */
        0x3b | 0x3a => 0x27,                               /* ; : */
        0x27 | 0x22 => 0x28,                               /* ' " */
        0x60 | 0x7e => 0x29,                               /* ` ~ */
        0x5c | 0x7c => 0x2b,                               /* \ | */
        0x2c | 0x3c => 0x33,                               /* , < */
        0x2e | 0x3e => 0x34,                               /* . > */
        0x2f | 0x3f => 0x35,                               /* / ? */
        0x20 => 0x39,                                      /* space */
        0xff1b => 0x01,                                    /* Escape */
        0xff08 => 0x0e,                                    /* BackSpace */
        0xff09 => 0x0f,                                    /* Tab */
        0xff0d => 0x1c,                                    /* Return */
        0xffe3 => 0x1d,                                    /* Control_L */
        0xffe1 => 0x2a,                                    /* Shift_L */
        0xffe2 => 0x36,                                    /* Shift_R */
        0xffe9 => 0x38,                                    /* Alt_L */
        0xffe5 => 0x3a,                                    /* Caps_Lock */
        0xffbe..=0xffc7 => (keysym - 0xffbe) as u8 + 0x3b, /* F1-F10 */
        0xffc8 => 0x57,                                    /* F11 */
        0xffc9 => 0x58,                                    /* F12 */
        _ => {
            let code = match keysym {
                0xffe4 => 0x1d,                            /* Control_R */
                0xffea | 0xfe03 => 0x38,                   /* Alt_R, ISO_Level3_Shift (AltGr) */
                0xff50 => 0x47,                            /* Home */
                0xff52 => 0x48,                            /* Up */
                0xff55 => 0x49,                            /* Page_Up */
                0xff51 => 0x4b,                            /* Left */
                0xff53 => 0x4d,                            /* Right */
                0xff57 => 0x4f,                            /* End */
                0xff54 => 0x50,                            /* Down */
                0xff56 => 0x51,                            /* Page_Down */
                0xff63 => 0x52,                            /* Insert */
                0xffff => 0x53,                            /* Delete */
                _ => return None
            };
            return Some((true, code));
        }
    };
    Some((false, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms() {
        assert_eq!(keysym_to_set1(0x61), Some((false, 0x1e)));   /* a */
        assert_eq!(keysym_to_set1(0x5a), Some((false, 0x2c)));   /* Z */
        assert_eq!(keysym_to_set1(0x6d), Some((false, 0x32)));   /* m */
        assert_eq!(keysym_to_set1(0x31), Some((false, 0x02)));   /* 1 */
        assert_eq!(keysym_to_set1(0x30), Some((false, 0x0b)));   /* 0 */
        assert_eq!(keysym_to_set1(0x21), Some((false, 0x02)));   /* ! */
        assert_eq!(keysym_to_set1(0xffbe), Some((false, 0x3b))); /* F1 */
        assert_eq!(keysym_to_set1(0xffc7), Some((false, 0x44))); /* F10 */
        assert_eq!(keysym_to_set1(0xff52), Some((true, 0x48)));  /* Up */
        assert_eq!(keysym_to_set1(0xffe4), Some((true, 0x1d)));  /* Control_R */
        assert_eq!(keysym_to_set1(0x1234), None);
    }
}
