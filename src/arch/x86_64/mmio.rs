use std::io::{self};
use crate::{ VM, MMIO, MMIODevice, GuestMemory };
use crate::{ read_le32, write_le32, read_le64, write_le64, gpa_to_host, read_guest_ptr };
use std::sync::{ Arc, Mutex };
use std::sync::atomic::{ Ordering, fence };
use std::ptr::read_volatile;

include!(concat!(env!("OUT_DIR"), "/virtio-blk-bindings.rs"));
include!(concat!(env!("OUT_DIR"), "/virtio-ring-bindings.rs"));

const VIRTIO_VERSION: u32 = 0x2;
const VIRTIO_ID_BLOCK: u32 = 0x2;
const VIRTIO_VENDOR_NONE: u32 = 0x0;

const VIRTIO_MMIO_MAGIC_VALUE: u64          = 0x000;
const VIRTIO_MMIO_VERSION: u64              = 0x004;
const VIRTIO_MMIO_DEVICE_ID: u64            = 0x008;
const VIRTIO_MMIO_VENDOR_ID: u64            = 0x00c;
const VIRTIO_MMIO_DEVICE_FEATURES: u64      = 0x010;
const VIRTIO_MMIO_DEVICE_FEATURES_SEL: u64  = 0x014;
const VIRTIO_MMIO_DRIVER_FEATURES: u64      = 0x020;
const VIRTIO_MMIO_DRIVER_FEATURES_SEL: u64  = 0x024;
const VIRTIO_MMIO_QUEUE_SEL: u64            = 0x030;
const VIRTIO_MMIO_QUEUE_NUM_MAX: u64        = 0x034;
const VIRTIO_MMIO_QUEUE_NUM: u64            = 0x038;
const VIRTIO_MMIO_QUEUE_READY: u64          = 0x044;
const VIRTIO_MMIO_QUEUE_NOTIFY: u64         = 0x050;
const VIRTIO_MMIO_STATUS: u64               = 0x070;
const VIRTIO_MMIO_QUEUE_DESC_LOW: u64       = 0x080;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: u64      = 0x084;
const VIRTIO_MMIO_QUEUE_AVAIL_LOW: u64      = 0x090;
const VIRTIO_MMIO_QUEUE_AVAIL_HIGH: u64     = 0x094;
const VIRTIO_MMIO_QUEUE_USED_LOW: u64       = 0x0a0;
const VIRTIO_MMIO_QUEUE_USED_HIGH: u64      = 0x0a4;
const VIRTIO_MMIO_CONFIG_GENERATION: u64    = 0x0fc;
const VIRTIO_MMIO_CONFIG: u64               = 0x100;

/* Taken from include/uapi/linux/virtio_blk.h */
const VIRTIO_BLK_F_SIZE_MAX: u32 =      1; /* Indicates maximum segment size */
const VIRTIO_BLK_F_SEG_MAX: u32 =       2; /* Indicates maximum # of segments */
const VIRTIO_BLK_F_GEOMETRY: u32 =      4; /* Legacy geometry available  */
const VIRTIO_BLK_F_RO: u32 =            5; /* Disk is read-only */
const VIRTIO_BLK_F_BLK_SIZE: u32 =      6; /* Block size of disk is available*/
const VIRTIO_BLK_F_TOPOLOGY: u32 =      10; /* Topology information is available */
const VIRTIO_BLK_F_MQ: u32 =            12; /* support more than one vq */
const VIRTIO_BLK_F_DISCARD: u32 =       13; /* DISCARD is supported */
const VIRTIO_BLK_F_WRITE_ZEROES: u32 =  14; /* WRITE ZEROES is supported */
const VIRTIO_BLK_F_SECURE_ERASE: u32 =  16;     /* Secure Erase is supported */
const VIRTIO_BLK_F_ZONED: u32 =         17; /* Zoned block device */

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

/* This marks a buffer as continuing via the next field. */
const VRING_DESC_F_NEXT: u16 =          1;
/* This marks a buffer as write-only (otherwise read-only). */
const VRING_DESC_F_WRITE: u16 =         2;
/* This means the buffer contains a list of buffer descriptors. */
const VRING_DESC_F_INDIRECT: u16 =      4;

/* The Host uses this in used->flags to advise the Guest: don't kick me when
 * you add a buffer.  It's unreliable, so it's simply an optimization.  Guest
 * will still kick if it's out of buffers. */
const VRING_USED_F_NO_NOTIFY: u16 =     1;
/* The Guest uses this in avail->flags to advise the Host: don't interrupt me
 * when you consume a buffer.  It's unreliable, so it's simply an
 * optimization.  */
const VRING_AVAIL_F_NO_INTERRUPT: u16 = 1;

pub enum DescType {
    WRITE,
    READ
}

impl vring_avail {
    fn needs_interrupt(&self) -> bool {
        self.flags & VRING_AVAIL_F_NO_INTERRUPT == 0
    }
}

impl vring_desc {
    fn type_(&self) -> DescType {
        match self.flags & VRING_DESC_F_WRITE {
            0 => DescType::READ,
            _ => DescType::WRITE
        }
    }

    fn is_indirect(&self) -> bool {
        self.flags & VRING_DESC_F_INDIRECT != 0
    }

    fn has_next(&self) -> bool {
        self.flags & VRING_DESC_F_NEXT != 0
    }

    fn get_data(&self,_mem: &GuestMemory) -> &[u8] {
        let ptr = gpa_to_host(&_mem, self.addr as u64, self.len as usize).unwrap();
        unsafe { std::slice::from_raw_parts(ptr, self.len as usize) }
    }
}

fn new_from<T>(bytes: &[u8], o: usize) -> Option<T> {
        let slice = bytes.get(o..o + std::mem::size_of::<T>())?;
        unsafe { Some(std::ptr::read_unaligned(slice.as_ptr() as *const T)) }
}

impl virtio_blk_outhdr {
    fn new_from(bytes: &[u8], o: usize) -> Self {
        new_from(bytes, o).unwrap()
    }
}

pub fn print_status(status: u32) {
    if status & VIRTIO_CONFIG_S_ACKNOWLEDGE != 0 {
        println!("Linux has seen the device and processed generic fields.");
    }
    if status & VIRTIO_CONFIG_S_DRIVER != 0 {
        println!("Kernel found a driver for the device.")
    }
    if status & VIRTIO_CONFIG_S_DRIVER_OK != 0 {
        println!("Driver has used its parts of the config, and is happy.");
    }
    if status & VIRTIO_CONFIG_S_FEATURES_OK != 0 {
        println!("Driver has finished configuring features.")
    }
    if status & VIRTIO_CONFIG_S_NEEDS_RESET != 0 {
        println!("Device entered invalid state, driver must reset it.");
    }
    if status & VIRTIO_CONFIG_S_FAILED != 0 {
        println!("Linux has given up on the device.");
    }
}

#[derive(Clone, Copy)]
pub struct VringQueue {
    ready: u32,
    num: u32,
    last_avail_idx: u16,
    desc: u64,
    avail: u64,
    used: u64
}

pub struct VirtioBlk {
    dev_feat_sel: u32,
    drv_feat_sel: u32,
    status: u32,
    curr_queue: u32,
    queue: [VringQueue; VRING_QUEUES_MAX]
}
const VIRTIO_MMIO_CONFIG_MAX: u64 = VIRTIO_MMIO_CONFIG + 4;

impl VringQueue {
    fn get_desc_head(&self, desc_idx: u16, _mem: &GuestMemory) -> vring_desc {
        let vring_desc_size = std::mem::size_of::<vring_desc>() as u64;
        let guest_addr = self.desc + (desc_idx as u64) * vring_desc_size;
        let desc_head_ptr = gpa_to_host(_mem, guest_addr, vring_desc_size as usize).unwrap();
        read_guest_ptr::<vring_desc>(desc_head_ptr)
    }
}

impl MMIODevice for VirtioBlk {
    fn handle(&mut self, mmio: &mut MMIO, _mem: &GuestMemory) -> io::Result<()> {
        let addr = mmio.phys_addr & 0xfff;
        let read = mmio.is_write == 0;
        match (read, addr) {
            (true, VIRTIO_MMIO_MAGIC_VALUE) => {
                let magic = read_le32(&['v', 'i', 'r', 't'].map(|c| c as u8), 0x0);
                write_le32(&mut mmio.data, 0, magic);
            },
            (true, VIRTIO_MMIO_VERSION) => {
                write_le32(&mut mmio.data, 0, VIRTIO_VERSION);
            },
            (true, VIRTIO_MMIO_DEVICE_ID) => {
                write_le32(&mut mmio.data, 0, VIRTIO_ID_BLOCK);
            },
            (true, VIRTIO_MMIO_VENDOR_ID) => {
                write_le32(&mut mmio.data, 0, VIRTIO_VENDOR_NONE);
            },
            (true, VIRTIO_MMIO_DEVICE_FEATURES) => {
                let features: u32 = match self.dev_feat_sel {
                    0 => 1 << VIRTIO_BLK_F_RO,
                    1 => 1 << (VIRTIO_F_VERSION_1 - 32),
                    _ => 0x0
                };
                write_le32(&mut mmio.data, 0, features);
            },
            (false, VIRTIO_MMIO_DEVICE_FEATURES) => {
                let features = read_le32(&mut mmio.data, 0);
                /* leave it like this for now */
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
            (false, VIRTIO_MMIO_DRIVER_FEATURES) => {
                let features = read_le32(&mut mmio.data, 0);
                /* also todo... */
            },
            (false, VIRTIO_MMIO_DRIVER_FEATURES_SEL) => {
                self.drv_feat_sel = read_le32(&mut mmio.data, 0);
                /* also leave it like this for now */
            },
            (false, VIRTIO_MMIO_QUEUE_SEL) => {
                self.curr_queue = read_le32(&mut mmio.data, 0);
            },
            (true, VIRTIO_MMIO_QUEUE_NUM_MAX) => {
                write_le32(&mut mmio.data, 0, if self.curr_queue == 0 { 128 } else { 0 });
            },
            (false, VIRTIO_MMIO_QUEUE_NUM) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.num = read_le32(&mut mmio.data, 0);
            },
            (false, VIRTIO_MMIO_QUEUE_READY) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.ready = read_le32(&mut mmio.data, 0);
            },
            (false, VIRTIO_MMIO_QUEUE_NOTIFY) => {
               let curr_queue = read_le32(&mut mmio.data, 0);
                let queue = &mut self.queue[curr_queue as usize];
                let vring_avail_raw_ptr = gpa_to_host(_mem, queue.avail, std::mem::size_of::<vring_avail>() + 2 * (queue.num as usize)).unwrap();
                unsafe {
                    let avail = read_guest_ptr::<vring_avail>(vring_avail_raw_ptr);
                    println!("flags: {:#x}\r", avail.flags);
                    println!("idx: {:#x}\r", avail.idx);
                    if avail.needs_interrupt() {
                        println!("NEEDS INTERRUPT");
                    }
                    fence(Ordering::Acquire);
                    while queue.last_avail_idx != avail.idx {
                        let mut write_buf: Vec<(*mut u8, u32)> = vec![];
                        let mut read_buf: Vec<u8> = vec![];
 
                        let avail_slot = ((queue.last_avail_idx as u32) % queue.num) as usize;
                        let mut desc_idx = read_guest_ptr::<u16>(vring_avail_raw_ptr.add(0x4 + 0x2 * avail_slot));
                        println!("last_avail_idx {:4}\r", queue.last_avail_idx);
                        for _ in 0..queue.num {
                            let desc_head = queue.get_desc_head(desc_idx, &_mem);
                            println!("    desc {:#x} {:4} {:#x} {:#x}\r",
                                desc_head.addr,
                                desc_head.len,
                                desc_head.flags,
                                desc_head.next);
                            match desc_head.type_() {
                                DescType::WRITE => write_buf.push((desc_head.addr as *mut u8, desc_head.len)),
                                DescType::READ => read_buf.extend_from_slice(desc_head.get_data(&_mem))
                            };
                            if !desc_head.has_next() {
                                break;
                            }
                            desc_idx = desc_head.next;
                        }
                        queue.last_avail_idx = queue.last_avail_idx.wrapping_add(1);
                        let outhdr = new_from::<virtio_blk_outhdr>(&read_buf, 0x0);
                        println!("type: {:#x}\r", outhdr.unwrap().type_);
                        println!("ioprio: {:4}\r", outhdr.unwrap().ioprio);
                        println!("sector: {:4}\r", outhdr.unwrap().sector);
                    }
               }
            },
            (true, VIRTIO_MMIO_QUEUE_READY) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                write_le32(&mut mmio.data, 0, queue.ready);
            },
            (true, VIRTIO_MMIO_STATUS) => {
                write_le32(&mut mmio.data, 0, self.status);
            },
            (false, VIRTIO_MMIO_STATUS) => {
                self.status = read_le32(&mut mmio.data, 0);
            },
            (false, VIRTIO_MMIO_QUEUE_DESC_LOW) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.desc = read_le32(&mut mmio.data, 0) as u64;
            },
            (false, VIRTIO_MMIO_QUEUE_DESC_HIGH) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.desc |= (read_le32(&mut mmio.data, 0) as u64) << 32;
            },
            (false, VIRTIO_MMIO_QUEUE_AVAIL_LOW) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.avail = read_le32(&mut mmio.data, 0) as u64;
            },
            (false, VIRTIO_MMIO_QUEUE_AVAIL_HIGH) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.avail |= (read_le32(&mut mmio.data, 0) as u64) << 32;
            },
            (false, VIRTIO_MMIO_QUEUE_USED_LOW) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.used = read_le32(&mut mmio.data, 0) as u64;
            },
            (false, VIRTIO_MMIO_QUEUE_USED_HIGH) => {
                let queue = &mut self.queue[self.curr_queue as usize];
                queue.used |= (read_le32(&mut mmio.data, 0) as u64) << 32;
            },
            (true, VIRTIO_MMIO_CONFIG_GENERATION) => {
                write_le32(&mut mmio.data, 0, 0x0);
            },
            (true, VIRTIO_MMIO_CONFIG..=VIRTIO_MMIO_CONFIG_MAX) => {
                match addr - VIRTIO_MMIO_CONFIG {
                    /* capacity is measured in 512 bytes */
                    0x0 => write_le32(&mut mmio.data, 0, 0x2), /* capacity low */
                    0x4 => write_le32(&mut mmio.data, 0, 0x0), /* capacity high */
                    _   => println!("Unknown config at {:#x}", addr)
                };
            },
            (_, _) => {
                println!("Got VIRTIO MMIO {} at {:#x}\r",
                if mmio.is_write != 0 { "write" } else { "read" }, mmio.phys_addr);
            }
        }
        Ok(())
    }
}

const VRING_QUEUES_MAX: usize = 2;

impl VirtioBlk {
    fn new() -> io::Result<Arc<Mutex<Self>>> {
        Ok(Arc::new(Mutex::new(Self {
            dev_feat_sel: 0,
            drv_feat_sel: 0,
            status: 0,
            curr_queue: 0,
            queue: [VringQueue::new(); VRING_QUEUES_MAX]
       })))
    }
}

impl VringQueue {
    fn new() -> Self {
        Self {
            desc: 0,
            avail: 0,
            last_avail_idx: 0,
            used: 0,
            ready: 0,
            num: 0
        }
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
        if let Some(device) = self.mmio_devices.get(&page) {
            device.lock().unwrap().handle(mmio, &self.guest_memory)?;
        }
        Ok(())
    }
}
