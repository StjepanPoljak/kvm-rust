use crate::{ VM, IO };
use std::io::{self};

impl VM {
    pub fn arch_handle_io(&mut self, io: &IO, base: *mut u8) -> io::Result<()> { Ok(()) } 

    pub fn arch_init_io_devices(&mut self) -> io::Result<()> { Ok(()) }
}

