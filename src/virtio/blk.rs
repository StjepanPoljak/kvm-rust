use std::io::{ self };
use crate::{ VirtioDevice, DescChain, write_le32, new_from };
use crate::virtio_bindings::*;
use std::fs::{ OpenOptions, File };
use std::os::unix::fs::FileExt;

pub struct VirtioBlk {
    disk: File
}

impl virtio_blk_outhdr {
    #[allow(dead_code)]
    fn new_from(bytes: &[u8], o: usize) -> Self {
        new_from(bytes, o).unwrap()
    }
}

impl VirtioBlk {
    pub fn new(disk: &str) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .open(disk)
            .expect(&format!("Failed to open {}.", disk));

        Ok(Self { disk: file })
    }
}

impl VirtioDevice for VirtioBlk {
    fn device_id(&self) -> u32 {
        VIRTIO_ID_BLOCK
    }

    fn features(&self, sel: u32) -> u32 {
        match sel {
            0 => 1 << VIRTIO_BLK_F_RO,
            _ => 0x0
        }
    }

    fn queue_num_max(&self, queue: u32) -> u32 {
        if queue == 0 { 128 } else { 0 }
    }

    fn read_config(&mut self, offset: u32, data: &mut [u8]) -> io::Result<()> {
        match offset {
            /* capacity is measured in 512 bytes */
            0x0 => write_le32(data, 0, (self.disk.metadata()?.len() / 512) as u32), /* capacity low */
            0x4 => write_le32(data, 0, 0x0), /* capacity high */
            _   => println!("Unknown config at {:#x}", offset)
        };
        Ok(())
    }

    fn process(&mut self, queue: u32, chain: &mut DescChain) -> io::Result<u32> {
        let outhdr = new_from::<virtio_blk_outhdr>(&chain.read_buf, 0x0)
            .ok_or(io::Error::other("Virtio-blk request without header."))?;
        let (status, data) = chain.write_buf.split_last_mut()
            .ok_or(io::Error::other("Virtio-blk request without status byte."))?;
        let mut offset = outhdr.sector * 512;
        let mut buf_num = 0;
        for buf in data.iter_mut() {
            self.disk.read_exact_at(buf, offset)?;
            offset += buf.len() as u64;
            buf_num += buf.len() as u64;
        }
        status[0] = VIRTIO_BLK_S_OK as u8;
        Ok(buf_num as u32)
    }
}
