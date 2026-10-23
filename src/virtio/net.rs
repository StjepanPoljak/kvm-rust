use std::io::{ self };
use crate::{ VirtioDevice, DescChain, new_from, kick_vcpu };
use crate::virtio_bindings::*;
use std::fs::{ OpenOptions, File };
use std::io::{ Read, Write };
use std::os::unix::io::AsRawFd;
use std::collections::VecDeque;
use std::sync::{ Arc, Mutex };

const VIRTIO_NET_QUEUE_RX: u32 = 0;
const VIRTIO_NET_QUEUE_TX: u32 = 1;

/* struct virtio_net_hdr_v1, prepended by the tap to every frame. */
const VIRTIO_NET_HDR_SIZE: libc::c_int = 12;
/* Header plus the largest frame the tap can hand us. */
const TAP_FRAME_MAX: usize = 65536 + VIRTIO_NET_HDR_SIZE as usize;
/* Frames are dropped beyond this while the guest posts no RX buffers. */
const RX_PENDING_MAX: usize = 256;

pub struct VirtioNet {
    tap: File,
    rx_pending: Arc<Mutex<VecDeque<Vec<u8>>>>
}

fn open_tap(name: &str) -> io::Result<File> {
    let tap = OpenOptions::new().read(true).write(true).open("/dev/net/tun")?;

    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    if name.len() >= ifr.ifr_name.len() {
        return Err(io::Error::other(format!("Tap name {} is too long.", name)));
    }
    for (d, s) in ifr.ifr_name.iter_mut().zip(name.bytes()) { *d = s as libc::c_char; }
    ifr.ifr_ifru.ifru_flags = (libc::IFF_TAP | libc::IFF_NO_PI | libc::IFF_VNET_HDR) as libc::c_short;

    if unsafe { libc::ioctl(tap.as_raw_fd(), libc::TUNSETIFF, &ifr) } < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::ioctl(tap.as_raw_fd(), libc::TUNSETVNETHDRSZ, &VIRTIO_NET_HDR_SIZE) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(tap)
}

/* Blocks on the tap and queues incoming frames until the guest takes them.
 * Kicks the vcpu only when the queue goes non-empty: while frames are still
 * pending, the guest refilling its RX buffers triggers the next delivery. */
fn start_rx_thread(mut tap: File, rx_pending: Arc<Mutex<VecDeque<Vec<u8>>>>) {
    std::thread::spawn(move || {
        let mut buf = vec![0u8; TAP_FRAME_MAX];
        loop {
            let len = match tap.read(&mut buf) {
                Ok(len) => len,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => { println!("Tap read failed: {}\r", e); break; }
            };
            let was_empty = {
                let mut pending = rx_pending.lock().unwrap();
                if pending.len() >= RX_PENDING_MAX {
                    continue;
                }
                pending.push_back(buf[..len].to_vec());
                pending.len() == 1
            };
            if was_empty {
                kick_vcpu();
            }
        }
    });
}

impl VirtioNet {
    pub fn new(tap_name: &str) -> io::Result<Self> {
        let tap = open_tap(tap_name)?;
        let rx_pending = Arc::new(Mutex::new(VecDeque::new()));
        start_rx_thread(tap.try_clone()?, rx_pending.clone());
        Ok(Self { tap, rx_pending })
    }
}

impl VirtioDevice for VirtioNet {
    fn device_id(&self) -> u32 {
        VIRTIO_ID_NET
    }

    fn features(&self, sel: u32) -> u32 {
        match sel {
            0 => 0x0,
            _ => 0x0
        }
    }

    fn queue_num_max(&self, queue: u32) -> u32 {
        match queue {
            0..=1 => 256,
            _ => 0
        }
    }

    fn read_config(&mut self, offset: u32, data: &mut [u8]) -> io::Result<()> {
        match offset {
            _   => println!("Unknown config at {:#x}", offset)
        };
        Ok(())
    }

    fn can_process(&self, queue: u32) -> bool {
        queue != VIRTIO_NET_QUEUE_RX || !self.rx_pending.lock().unwrap().is_empty()
    }

    fn process(&mut self, queue: u32, chain: &mut DescChain) -> io::Result<u32> {
        match queue {
            VIRTIO_NET_QUEUE_RX => {
                /* The tap already prepended the virtio_net_hdr, so the frame goes in as is. */
                let Some(frame) = self.rx_pending.lock().unwrap().pop_front() else {
                    return Ok(0x0);
                };
                let mut copied = 0;
                for buf in chain.write_buf.iter_mut() {
                    if copied == frame.len() {
                        break;
                    }
                    let len = buf.len().min(frame.len() - copied);
                    buf[..len].copy_from_slice(&frame[copied..copied + len]);
                    copied += len;
                }
                if copied < frame.len() {
                    println!("Virtio-net RX buffer too small, frame truncated.\r");
                }
                return Ok(copied as u32);
            },
            VIRTIO_NET_QUEUE_TX => {
                new_from::<virtio_net_hdr_v1>(&chain.read_buf, 0x0)
                    .ok_or(io::Error::other("Virtio-net request without header."))?;
                /* The tap takes the virtio_net_hdr in front of the frame. A failed
                 * write (e.g. tap link down) drops the frame instead of the VM. */
                if let Err(e) = self.tap.write_all(&chain.read_buf) {
                    println!("Tap write failed: {}\r", e);
                }
            },
            _ => ()
        }
        Ok(0x0)
    }
}
