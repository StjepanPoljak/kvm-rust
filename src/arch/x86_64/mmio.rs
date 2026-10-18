use std::io::{ self };
use crate::{ VM, MMIO, MMIODevice, GuestMemory, Args };
use crate::{ read_le32, write_le32, gpa_to_host, read_guest_ptr, write_guest_ptr };
use super::arch_update_irq;
use std::sync::{ Arc, Mutex };
use std::sync::atomic::{ Ordering, fence };
use std::fs::{ OpenOptions, File };
use std::os::unix::fs::FileExt;

#[allow(non_camel_case_types, non_upper_case_globals, non_snake_case, dead_code)]
mod virtio_bindings {
    include!(concat!(env!("OUT_DIR"), "/virtio-blk-bindings.rs"));
    include!(concat!(env!("OUT_DIR"), "/virtio-ring-bindings.rs"));
}
use virtio_bindings::*;

const VIRTIO_VERSION: u32 = 0x2;
const VIRTIO_VENDOR_NONE: u32 = 0x0;

pub enum DescType {
    WRITE,
    READ
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
    queue: [VringQueue; VRING_QUEUES_MAX],
    disk: File,
    vm_fd: libc::c_int,
    irq_status: u32
}
const VIRTIO_MMIO_CONFIG_MAX: u32 = VIRTIO_MMIO_CONFIG + 4;

impl vring_avail {
    fn needs_interrupt(&self) -> bool {
        (self.flags as u32) & VRING_AVAIL_F_NO_INTERRUPT == 0
    }
}

impl vring_desc {
    fn type_(&self) -> DescType {
        match (self.flags as u32) & VRING_DESC_F_WRITE {
            0 => DescType::READ,
            _ => DescType::WRITE
        }
    }

    #[allow(dead_code)]
    fn is_indirect(&self) -> bool {
        (self.flags as u32) & VRING_DESC_F_INDIRECT != 0
    }

    fn has_next(&self) -> bool {
        (self.flags as u32) & VRING_DESC_F_NEXT != 0
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
    #[allow(dead_code)]
    fn new_from(bytes: &[u8], o: usize) -> Self {
        new_from(bytes, o).unwrap()
    }
}

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
        let addr = (mmio.phys_addr & 0xfff) as u32;
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
                let _features = read_le32(&mut mmio.data, 0);
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
                let _features = read_le32(&mut mmio.data, 0);
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
                let vring_avail_raw_ptr = gpa_to_host(_mem, queue.avail, std::mem::size_of::<vring_avail>() + std::mem::size_of::<u16>() * (queue.num as usize)).unwrap();
                let vring_used_raw_ptr = gpa_to_host(_mem, queue.used, std::mem::size_of::<vring_used>() + std::mem::size_of::<vring_used_elem>() * (queue.num as usize)).unwrap();
                unsafe {
                    let avail = read_guest_ptr::<vring_avail>(vring_avail_raw_ptr);
                    fence(Ordering::Acquire);
                    while queue.last_avail_idx != avail.idx {
                        let mut write_buf: Vec<&mut [u8]> = vec![];
                        let mut read_buf: Vec<u8> = vec![];
 
                        let avail_slot = ((queue.last_avail_idx as u32) % queue.num) as usize;
                        let desc_head_idx = read_guest_ptr::<u16>(vring_avail_raw_ptr.add(0x4 + 0x2 * avail_slot));
                        let mut desc_idx = desc_head_idx;
                        for _ in 0..queue.num {
                            let desc_head = queue.get_desc_head(desc_idx, &_mem);
                            match desc_head.type_() {
                                DescType::WRITE => {
                                    let ptr = gpa_to_host(&_mem, desc_head.addr as u64, desc_head.len as usize).unwrap();
                                    unsafe {
                                        write_buf.push(std::slice::from_raw_parts_mut(ptr as *mut u8, desc_head.len as usize));
                                    }
                                },
                                DescType::READ => read_buf.extend_from_slice(desc_head.get_data(&_mem))
                            };
                            if !desc_head.has_next() {
                                break;
                            }
                            desc_idx = desc_head.next;
                        }
                        queue.last_avail_idx = queue.last_avail_idx.wrapping_add(1);
                        let outhdr = new_from::<virtio_blk_outhdr>(&read_buf, 0x0).unwrap();
                        let (status, data) = write_buf.split_last_mut().unwrap();
                        let mut offset = outhdr.sector * 512;
                        let mut buf_num = 0;
                        for buf in data.iter_mut() {
                            self.disk.read_exact_at(buf, offset)?;
                            offset += buf.len() as u64;
                            buf_num += buf.len() as u64;
                        }
                        let used = read_guest_ptr::<vring_used>(vring_used_raw_ptr);
                        let used_slot = ((used.idx as u32) % queue.num) as usize;
                        write_guest_ptr::<vring_used_elem>(vring_used_raw_ptr.add(0x4 + std::mem::size_of::<vring_used_elem>() * used_slot), vring_used_elem { id: desc_head_idx as u32, len: buf_num as u32});
                        status[0] = VIRTIO_BLK_S_OK as u8;

                        fence(Ordering::Release);
                        write_guest_ptr::<u16>(vring_used_raw_ptr.add(0x2), used.idx + 1);

                        self.irq_status |= VIRTIO_MMIO_INT_VRING;
                        arch_update_irq(1, self.vm_fd, 0x5)?;
                    }
               }
            },
            (true, VIRTIO_MMIO_INTERRUPT_STATUS) => {
                write_le32(&mut mmio.data, 0, self.irq_status);
            },
            (false, VIRTIO_MMIO_INTERRUPT_ACK) => {
                self.irq_status &= !read_le32(&mut mmio.data, 0);
                arch_update_irq(0, self.vm_fd, 0x5)?;
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
                    0x0 => write_le32(&mut mmio.data, 0, (self.disk.metadata()?.len() / 512) as u32), /* capacity low */
                    0x4 => write_le32(&mut mmio.data, 0, 0x0), /* capacity high */
                    _   => println!("Unknown config at {:#x}", addr)
                };
            },
            (_, _) => ()
        }
        Ok(())
    }
}

const VRING_QUEUES_MAX: usize = 2;

impl VirtioBlk {
    fn new(disk: &str, vm_fd: libc::c_int) -> io::Result<Arc<Mutex<Self>>> {
        let file = OpenOptions::new()
            .read(true)
            .open(disk)
            .expect(&format!("Failed to open {}.", disk));

        Ok(Arc::new(Mutex::new(Self {
            dev_feat_sel: 0,
            drv_feat_sel: 0,
            status: 0,
            curr_queue: 0,
            queue: [VringQueue::new(); VRING_QUEUES_MAX],
            disk: file,
            vm_fd: vm_fd,
            irq_status: 0
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
    pub fn arch_init_mmio_devices(&mut self, args: &Args) -> io::Result<()> {
        if let Some(disk) = &args.disk {
            let virtio_blk = VirtioBlk::new(disk, self.fd)?;
            self.mmio_devices.insert(0xd0000, virtio_blk.clone() as Arc<Mutex<dyn MMIODevice>>);
        }
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
