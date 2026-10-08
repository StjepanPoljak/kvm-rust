use std::io::{self};
use crate::{ VM, MMIO, MMIODevice };
use crate::{ read_le32, write_le32, read_le64, write_le64 };
use std::sync::{ Arc, Mutex };

const VIRTIO_VERSION: u32 = 0x2;
const VIRTIO_ID_BLOCK: u32 = 0x2;
const VIRTIO_VENDOR_NONE: u32 = 0x0;

const VIRTIO_MMIO_MAGIC_VALUE: u64 =	0x000;
const VIRTIO_MMIO_VERSION: u64 =	0x004;
const VIRTIO_MMIO_DEVICE_ID: u64 =	0x008;
const VIRTIO_MMIO_VENDOR_ID: u64 =	0x00c;


const VIRTIO_MMIO_DEVICE_FEATURES: u64 = 0x10;
const VIRTIO_MMIO_DEVICE_FEATURES_SEL: u64 = 0x14;
const VIRTIO_MMIO_DRIVER_FEATURES: u64 = 0x20;
const VIRTIO_MMIO_DRIVER_FEATURES_SEL: u64 = 0x24;
const VIRTIO_MMIO_QUEUE_SEL: u64 = 0x30;
const VIRTIO_MMIO_QUEUE_NUM_MAX: u64 = 0x34;
const VIRTIO_MMIO_QUEUE_NUM: u64 = 0x38;
const VIRTIO_MMIO_QUEUE_READY: u64 = 0x44;

/* Taken from include/uapi/linux/virtio_blk.h */
const VIRTIO_BLK_F_SIZE_MAX: u32 =	1;	/* Indicates maximum segment size */
const VIRTIO_BLK_F_SEG_MAX: u32 =	2;	/* Indicates maximum # of segments */
const VIRTIO_BLK_F_GEOMETRY: u32 =	4;	/* Legacy geometry available  */
const VIRTIO_BLK_F_RO: u32 =		5;	/* Disk is read-only */
const VIRTIO_BLK_F_BLK_SIZE: u32 =	6;	/* Block size of disk is available*/
const VIRTIO_BLK_F_TOPOLOGY: u32 =	10;	/* Topology information is available */
const VIRTIO_BLK_F_MQ: u32 =            12;	/* support more than one vq */
const VIRTIO_BLK_F_DISCARD: u32 =       13;	/* DISCARD is supported */
const VIRTIO_BLK_F_WRITE_ZEROES: u32 =	14;	/* WRITE ZEROES is supported */
const VIRTIO_BLK_F_SECURE_ERASE: u32 =	16;     /* Secure Erase is supported */
const VIRTIO_BLK_F_ZONED: u32 =		17;	/* Zoned block device */

const VIRTIO_F_VERSION_1: u32 =         32;

/* Taken from include/uapi/linux/virtio_config.h */
/* We have seen device and processed generic fields (VIRTIO_CONFIG_F_VIRTIO) */
const VIRTIO_CONFIG_S_ACKNOWLEDGE: u32 =    1;
/* We have found a driver for the device. */
const VIRTIO_CONFIG_S_DRIVER: u32 =         2;
/* Driver has used its parts of the config, and is happy */
const VIRTIO_CONFIG_S_DRIVER_OK: u32 =      4;
/* Driver has finished configuring features */
const VIRTIO_CONFIG_S_FEATURES_OK: u32 =    8;
/* Device entered invalid state, driver must reset it */
const VIRTIO_CONFIG_S_NEEDS_RESET: u32 =    0x40;
/* We've given up on this device. */
const VIRTIO_CONFIG_S_FAILED: u32 =         0x80;

pub struct VirtioBlk {
    dev_feat_sel: u32,
    drv_feat_sel: u32,
    status: u32
}

impl MMIODevice for VirtioBlk {
    fn handle(&mut self, mmio: &mut MMIO) -> io::Result<()> {
        let addr = mmio.phys_addr & 0xfff;
        let read = mmio.is_write == 0;
        match (read, addr) {
            (true, VIRTIO_MMIO_MAGIC_VALUE) => {
                let magic = read_le32(&[ 'v', 'i', 'r', 't' ].map(|c| c as u8), 0x0);
                write_le32(&mut mmio.data, 0, magic);
            },
            (true, VIRTIO_MMIO_VERSION) => write_le32(&mut mmio.data, 0, VIRTIO_VERSION),
            (true, VIRTIO_MMIO_DEVICE_ID) => write_le32(&mut mmio.data, 0, VIRTIO_ID_BLOCK),
            (true, VIRTIO_MMIO_VENDOR_ID) => write_le32(&mut mmio.data, 0, VIRTIO_VENDOR_NONE),
            (true, VIRTIO_MMIO_DEVICE_FEATURES) => {
                let features: u32 = match self.dev_feat_sel {
                    0 => 0x0, //(1 << VIRTIO_BLK_F_RO),
                    1 => 1 << (VIRTIO_F_VERSION_1 - 32),
                    _ => 0x0
                };
                write_le32(&mut mmio.data, 0, features);
            },
            (false, VIRTIO_MMIO_DEVICE_FEATURES_SEL) => {
                self.dev_feat_sel = read_le32(&mut mmio.data, 0);
            },
            (true, VIRTIO_MMIO_DRIVER_FEATURES) => {
                let features: u32 = match self.drv_feat_sel {
                    0 => 0x0,
                    1 => 0x0,
                    _ => 0x0
                };
                write_le32(&mut mmio.data, 0, features);
            },
            (false, VIRTIO_MMIO_DRIVER_FEATURES_SEL) => {
                self.drv_feat_sel = read_le32(&mut mmio.data, 0);
            },
            (true, 0x6f) => {
                write_le32(&mut mmio.data, 0, self.status);
            },
            (false, 0x6f) => {
                self.status = read_le32(&mut mmio.data, 0);
            },
            (_, _) => ()
        }
        println!("Got VIRTIO MMIO {} at {:#x}",
            if mmio.is_write != 0 { "write" } else { "read" }, mmio.phys_addr);
        Ok(())
    }
}

impl VirtioBlk {
    fn new() -> io::Result<Arc<Mutex<Self>>> {
        Ok(Arc::new(Mutex::new(Self { dev_feat_sel: 0, drv_feat_sel: 0, status: 0 })))
    }
}

impl VM {
     pub fn arch_init_mmio_devices(&mut self) -> io::Result<()> {
        let virtio_blk = VirtioBlk::new()?;
        self.mmio_devices.insert(0xd0000, virtio_blk.clone() as Arc<Mutex<dyn MMIODevice>>);
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
