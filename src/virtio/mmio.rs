use std::io::{ self };
use crate::{ MMIO, MMIODevice, GuestMemory, VringQueue, DescChain, VRING_QUEUES_MAX };
use crate::{ read_le32, write_le32 };
use crate::arch::arch_update_irq;
use crate::virtio_bindings::*;
use std::sync::{ Arc, Mutex };

const VIRTIO_VERSION: u32 = 0x2;
const VIRTIO_VENDOR_NONE: u32 = 0x0;
const VIRTIO_MMIO_CONFIG_MAX: u32 = 0xfff;

/* Device-specific part of a virtio device; VirtioMmio is the transport. */
pub trait VirtioDevice {
    fn device_id(&self) -> u32;
    /* Device feature bits for feature word `sel`, transport bits excluded. */
    fn features(&self, sel: u32) -> u32;
    /* 0 means the queue does not exist. */
    fn queue_num_max(&self, queue: u32) -> u32;
    fn read_config(&mut self, offset: u32, data: &mut [u8]) -> io::Result<()>;
    /* Handles one request and returns the number of bytes written to the guest. */
    fn process(&mut self, queue: u32, chain: &mut DescChain) -> io::Result<u32>;
    /* False leaves available chains in the queue, e.g. RX buffers with no frame to fill them. */
    fn can_process(&self, _queue: u32) -> bool { true }
}

pub struct VirtioMmio<D: VirtioDevice> {
    dev_feat_sel: u32,
    drv_feat_sel: u32,
    status: u32,
    curr_queue: u32,
    queue: [VringQueue; VRING_QUEUES_MAX],
    vm_fd: libc::c_int,
    irq: u32,
    irq_status: u32,
    device: D
}

impl<D: VirtioDevice> VirtioMmio<D> {
    pub fn new(device: D, vm_fd: libc::c_int, irq: u32) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            dev_feat_sel: 0,
            drv_feat_sel: 0,
            status: 0,
            curr_queue: 0,
            queue: [VringQueue::new(); VRING_QUEUES_MAX],
            vm_fd: vm_fd,
            irq: irq,
            irq_status: 0,
            device: device
        }))
    }

    fn selected_queue(&mut self) -> Option<&mut VringQueue> {
        self.queue.get_mut(self.curr_queue as usize)
    }

    fn queue_notify(&mut self, queue_idx: u32, mem: &GuestMemory) -> io::Result<()> {
        let Some(queue) = self.queue.get_mut(queue_idx as usize) else {
            return Ok(());
        };
        if queue.ready == 0 || queue.num == 0 {
            return Ok(());
        }
        while self.device.can_process(queue_idx) {
            let Some(mut chain) = queue.pop_avail(mem)? else { break; };
            let len = self.device.process(queue_idx, &mut chain)?;
            queue.push_used(mem, chain.head, len)?;

            self.irq_status |= VIRTIO_MMIO_INT_VRING;
            arch_update_irq(1, self.vm_fd, self.irq)?;
        }
        Ok(())
    }
}

impl<D: VirtioDevice> MMIODevice for VirtioMmio<D> {
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
                write_le32(&mut mmio.data, 0, self.device.device_id());
            },
            (true, VIRTIO_MMIO_VENDOR_ID) => {
                write_le32(&mut mmio.data, 0, VIRTIO_VENDOR_NONE);
            },
            (true, VIRTIO_MMIO_DEVICE_FEATURES) => {
                let transport: u32 = match self.dev_feat_sel {
                    1 => 1 << (VIRTIO_F_VERSION_1 - 32),
                    _ => 0x0
                };
                write_le32(&mut mmio.data, 0, transport | self.device.features(self.dev_feat_sel));
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
                let num_max = if (self.curr_queue as usize) < VRING_QUEUES_MAX {
                    self.device.queue_num_max(self.curr_queue)
                } else { 0 };
                write_le32(&mut mmio.data, 0, num_max);
            },
            (false, VIRTIO_MMIO_QUEUE_NUM) => {
                let val = read_le32(&mut mmio.data, 0);
                if let Some(queue) = self.selected_queue() { queue.num = val; }
            },
            (false, VIRTIO_MMIO_QUEUE_READY) => {
                let val = read_le32(&mut mmio.data, 0);
                if let Some(queue) = self.selected_queue() { queue.ready = val; }
            },
            (false, VIRTIO_MMIO_QUEUE_NOTIFY) => {
                let queue_idx = read_le32(&mut mmio.data, 0);
                self.queue_notify(queue_idx, _mem)?;
            },
            (true, VIRTIO_MMIO_INTERRUPT_STATUS) => {
                write_le32(&mut mmio.data, 0, self.irq_status);
            },
            (false, VIRTIO_MMIO_INTERRUPT_ACK) => {
                self.irq_status &= !read_le32(&mut mmio.data, 0);
                arch_update_irq(0, self.vm_fd, self.irq)?;
            },
            (true, VIRTIO_MMIO_QUEUE_READY) => {
                let ready = self.selected_queue().map_or(0, |queue| queue.ready);
                write_le32(&mut mmio.data, 0, ready);
            },
            (true, VIRTIO_MMIO_STATUS) => {
                write_le32(&mut mmio.data, 0, self.status);
            },
            (false, VIRTIO_MMIO_STATUS) => {
                self.status = read_le32(&mut mmio.data, 0);
                if self.status & VIRTIO_CONFIG_S_FAILED != 0 {
                    println!("Warning: Virtio config failed!\r");
                }
            },
            (false, VIRTIO_MMIO_QUEUE_DESC_LOW) => {
                let val = read_le32(&mut mmio.data, 0) as u64;
                if let Some(queue) = self.selected_queue() { queue.desc = val; }
            },
            (false, VIRTIO_MMIO_QUEUE_DESC_HIGH) => {
                let val = read_le32(&mut mmio.data, 0) as u64;
                if let Some(queue) = self.selected_queue() { queue.desc |= val << 32; }
            },
            (false, VIRTIO_MMIO_QUEUE_AVAIL_LOW) => {
                let val = read_le32(&mut mmio.data, 0) as u64;
                if let Some(queue) = self.selected_queue() { queue.avail = val; }
            },
            (false, VIRTIO_MMIO_QUEUE_AVAIL_HIGH) => {
                let val = read_le32(&mut mmio.data, 0) as u64;
                if let Some(queue) = self.selected_queue() { queue.avail |= val << 32; }
            },
            (false, VIRTIO_MMIO_QUEUE_USED_LOW) => {
                let val = read_le32(&mut mmio.data, 0) as u64;
                if let Some(queue) = self.selected_queue() { queue.used = val; }
            },
            (false, VIRTIO_MMIO_QUEUE_USED_HIGH) => {
                let val = read_le32(&mut mmio.data, 0) as u64;
                if let Some(queue) = self.selected_queue() { queue.used |= val << 32; }
            },
            (true, VIRTIO_MMIO_CONFIG_GENERATION) => {
                write_le32(&mut mmio.data, 0, 0x0);
            },
            (true, VIRTIO_MMIO_CONFIG..=VIRTIO_MMIO_CONFIG_MAX) => {
                self.device.read_config(addr - VIRTIO_MMIO_CONFIG, &mut mmio.data)?;
            },
            (_, _) => {
           }
        }
        Ok(())
    }

    fn poll(&mut self, mem: &GuestMemory) -> io::Result<()> {
        for queue_idx in 0..VRING_QUEUES_MAX as u32 {
            self.queue_notify(queue_idx, mem)?;
        }
        Ok(())
    }
}
