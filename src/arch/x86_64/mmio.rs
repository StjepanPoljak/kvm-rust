use std::io::{ self };
use crate::{ VM, Args };

pub const VIRTIO_BLK_MMIO_ADDR: u64 = 0xd000_0000;
pub const VIRTIO_BLK_IRQ: u32 = 0x5;

impl VM {
    pub fn arch_init_mmio_devices(&mut self, args: &Args) -> io::Result<()> {
        self.init_virtio_blk(args, VIRTIO_BLK_MMIO_ADDR, VIRTIO_BLK_IRQ)
    }
}
