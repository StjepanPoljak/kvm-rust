use std::io::{ self };
use crate::{ VirtioDevice, DescChain, write_le32, new_from };
use crate::virtio_bindings::*;
use std::fs::{ OpenOptions, File };
use std::os::unix::fs::FileExt;

const SECTOR_SIZE: u64 = 512;

pub struct VirtioBlk {
    disk: File,
    /* in sectors */
    capacity: u64
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
            .write(true)
            .open(disk)
            .map_err(|e| io::Error::new(e.kind(), format!("Failed to open {}: {}", disk, e)))?;
        let capacity = file.metadata()?.len() / SECTOR_SIZE;

        Ok(Self { disk: file, capacity })
    }

    /* Checks that `len` bytes from `sector` lie within the disk, returns the byte offset. */
    fn disk_offset(&self, sector: u64, len: usize) -> io::Result<u64> {
        let end = sector.checked_mul(SECTOR_SIZE)
            .and_then(|offset| offset.checked_add(len as u64).map(|end| (offset, end)));
        match end {
            Some((offset, end)) if end <= self.capacity * SECTOR_SIZE => Ok(offset),
            _ => Err(io::Error::other(format!("Virtio-blk access past end of disk (sector {:#x}, {} bytes).", sector, len)))
        }
    }

    fn read(&self, sector: u64, data: &mut [&mut [u8]]) -> io::Result<u32> {
        let total = data.iter().map(|buf| buf.len()).sum();
        let mut offset = self.disk_offset(sector, total)?;
        for buf in data.iter_mut() {
            self.disk.read_exact_at(buf, offset)?;
            offset += buf.len() as u64;
        }
        Ok(total as u32)
    }

    fn write(&self, sector: u64, data: &[u8]) -> io::Result<()> {
        let offset = self.disk_offset(sector, data.len())?;
        self.disk.write_all_at(data, offset)
    }
}

impl VirtioDevice for VirtioBlk {
    fn device_id(&self) -> u32 {
        VIRTIO_ID_BLOCK
    }

    fn features(&self, sel: u32) -> u32 {
        match sel {
            0 => 1 << VIRTIO_BLK_F_FLUSH,
            _ => 0x0
        }
    }

    fn queue_num_max(&self, queue: u32) -> u32 {
        if queue == 0 { 128 } else { 0 }
    }

    fn read_config(&mut self, offset: u32, data: &mut [u8]) -> io::Result<()> {
        match offset {
            /* capacity is measured in 512 bytes */
            0x0 => write_le32(data, 0, self.capacity as u32), /* capacity low */
            0x4 => write_le32(data, 0, (self.capacity >> 32) as u32), /* capacity high */
            _   => println!("Unknown config at {:#x}", offset)
        };
        Ok(())
    }

    /* Disk errors go back to the guest as VIRTIO_BLK_S_IOERR; only a
     * malformed request is an error for the VMM. */
    fn process(&mut self, _queue: u32, chain: &mut DescChain) -> io::Result<u32> {
        let outhdr = new_from::<virtio_blk_outhdr>(&chain.read_buf, 0x0)
            .ok_or(io::Error::other("Virtio-blk request without header."))?;
        let (status, data) = chain.write_buf.split_last_mut()
            .ok_or(io::Error::other("Virtio-blk request without status byte."))?;
        let out_data = &chain.read_buf[std::mem::size_of::<virtio_blk_outhdr>()..];

        let (res, written) = match outhdr.type_ {
            VIRTIO_BLK_T_IN => match self.read(outhdr.sector, data) {
                Ok(len) => (Ok(()), len),
                Err(e) => (Err(e), 0)
            },
            VIRTIO_BLK_T_OUT => (self.write(outhdr.sector, out_data), 0),
            VIRTIO_BLK_T_FLUSH => (self.disk.sync_data(), 0),
            _ => {
                status[0] = VIRTIO_BLK_S_UNSUPP as u8;
                return Ok(1);
            }
        };

        status[0] = match res {
            Ok(()) => VIRTIO_BLK_S_OK,
            Err(e) => { println!("Virtio-blk request failed: {}\r", e); VIRTIO_BLK_S_IOERR }
        } as u8;
        /* The used length counts every byte written to the guest, status included. */
        Ok(written + 1)
    }
}
