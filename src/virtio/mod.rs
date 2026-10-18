#[allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code)]
mod virtio_bindings {
    include!(concat!(env!("OUT_DIR"), "/virtio-blk-bindings.rs"));
    include!(concat!(env!("OUT_DIR"), "/virtio-ring-bindings.rs"));
}

mod queue;
mod mmio;
mod blk;

pub use queue::*;
pub use mmio::*;
pub use blk::*;

impl VM {
    pub fn init_virtio_blk(&mut self, args: &Args, addr: u64, irq: u32) -> io::Result<()> {
        if let Some(disk) = &args.disk {
            let virtio_blk = VirtioMmio::new(VirtioBlk::new(disk)?, self.fd, irq);
            self.add_mmio_device(addr, virtio_blk);
        }
        Ok(())
    }
}
