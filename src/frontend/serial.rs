use std::io::{ self };
use termion::raw::IntoRawMode;
use std::io::{ stdin, stdout, Read };
use crate::VmControl;
use std::sync::{ Arc, Mutex };

pub trait TTY: Send {
    fn send_char(&mut self, ch: u8) -> io::Result<()>;
}

pub fn save_tty() -> Option<libc::termios> {
    let mut tty: libc::termios = unsafe { std::mem::zeroed() };
    (unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut tty) } == 0).then_some(tty)
}

pub fn restore_tty(tty: &libc::termios) {
    unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, tty); }
}

pub fn start_tty(uart_async: Arc<Mutex<dyn TTY>>, control: Arc<VmControl>) {
    std::thread::spawn(move || {
        let mut is_escape = false;
        let raw = stdout().into_raw_mode().unwrap();
        const QUIT_BYTE : u8 = 'x' as u8;

        for byte in stdin().bytes() {
            let b = byte.unwrap();

            if !is_escape && b == 0x01 {
                is_escape = true;
                continue;
            } else if is_escape {
                is_escape = false;
                match b {
                    QUIT_BYTE => { break; },
                    0x01 => (),
                    _ => { continue; }
                };
            }

            uart_async.lock().unwrap().send_char(b).unwrap();
        }

        drop(raw);
        control.stop();
    });
}
