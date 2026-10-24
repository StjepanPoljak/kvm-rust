use std::io::{ self };
use std::sync::{ Arc, Mutex };
use crate::{ IO, IODevice, Display, read_guest_ptr, KVM_EXIT_IO_IN, KVM_EXIT_IO_OUT };

/* A VGA adapter showing mode 13h: 320x200, a byte per pixel indexing
 * the 256-colour DAC palette, framebuffer linear at 0xA0000. The
 * framebuffer is plain guest RAM, so drawing costs no exits; only the
 * registers are trapped. They are stored and read back but not
 * interpreted: whatever mode the guest sets, the frontends get mode 13h,
 * blanked while the attribute controller has the display off. */

pub const VGA_PORTS: std::ops::RangeInclusive<u16> = 0x3c0..=0x3df;
pub const VGA_FRAMEBUFFER: u64 = 0xa0000;
const WIDTH: usize = 320;
const HEIGHT: usize = 200;
pub const VGA_FRAMEBUFFER_SIZE: usize = WIDTH * HEIGHT;

const AC_INDEX: u16 = 0x3c0;        /* write: index and data in turn */
const AC_READ: u16 = 0x3c1;
const MISC_WRITE: u16 = 0x3c2;
const SEQ_INDEX: u16 = 0x3c4;
const SEQ_DATA: u16 = 0x3c5;
const DAC_READ_INDEX: u16 = 0x3c7;  /* read: the DAC state */
const DAC_WRITE_INDEX: u16 = 0x3c8;
const DAC_DATA: u16 = 0x3c9;
const MISC_READ: u16 = 0x3cc;
const GC_INDEX: u16 = 0x3ce;
const GC_DATA: u16 = 0x3cf;
const CRTC_INDEX: u16 = 0x3d4;
const CRTC_DATA: u16 = 0x3d5;
const INSTAT_READ: u16 = 0x3da;     /* also resets the AC flip-flop */

/* Attribute controller index: palette address source, the display is
 * on only while it is set. */
const AC_PAS: u8 = 0x20;
/* Input status 1: display disabled, vertical retrace */
const INSTAT_DD: u8 = 0x01;
const INSTAT_VR: u8 = 0x08;
const DAC_STATE_READ: u8 = 0x03;

pub struct Vga {
    misc: u8,
    seq_index: u8,
    seq: [u8; 8],
    gc_index: u8,
    gc: [u8; 16],
    crtc_index: u8,
    crtc: [u8; 32],
    /* The index byte also carries AC_PAS. */
    ac_index: u8,
    ac_data_next: bool,
    ac: [u8; 32],
    /* 6 bits per component, as the guest wrote them */
    palette: [[u8; 3]; 256],
    dac_write: (u8, usize),
    dac_read: (u8, usize),
    dac_reading: bool,
    /* Toggled by every read of INSTAT_READ: there is no real refresh,
     * so a guest waiting for retrace sees one soon. */
    retrace: bool,
    /* None when guest RAM does not cover the framebuffer */
    framebuffer: Option<*const u8>
}

/* The framebuffer points into a MemRegion, which outlives the devices
 * and is written by the guest concurrently anyway; see MemRegion. */
unsafe impl Send for Vga {}

impl Vga {
    pub fn new(framebuffer: Option<*const u8>) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            misc: 0,
            seq_index: 0,
            seq: [0; 8],
            gc_index: 0,
            gc: [0; 16],
            crtc_index: 0,
            crtc: [0; 32],
            ac_index: 0,
            ac_data_next: false,
            ac: [0; 32],
            palette: [[0; 3]; 256],
            dac_write: (0, 0),
            dac_read: (0, 0),
            dac_reading: false,
            retrace: false,
            framebuffer
        }))
    }

    fn write(&mut self, port: u16, byte: u8) {
        match port {
            AC_INDEX => {
                if self.ac_data_next {
                    self.ac[(self.ac_index & 0x1f) as usize] = byte;
                } else {
                    self.ac_index = byte & 0x3f;
                }
                self.ac_data_next = !self.ac_data_next;
            },
            MISC_WRITE => self.misc = byte,
            SEQ_INDEX => self.seq_index = byte & 0x07,
            SEQ_DATA => self.seq[self.seq_index as usize] = byte,
            DAC_READ_INDEX => {
                self.dac_read = (byte, 0);
                self.dac_reading = true;
            },
            DAC_WRITE_INDEX => {
                self.dac_write = (byte, 0);
                self.dac_reading = false;
            },
            DAC_DATA => {
                let (index, component) = self.dac_write;
                self.palette[index as usize][component] = byte & 0x3f;
                self.dac_write = if component == 2 { (index.wrapping_add(1), 0) } else { (index, component + 1) };
            },
            GC_INDEX => self.gc_index = byte & 0x0f,
            GC_DATA => self.gc[self.gc_index as usize] = byte,
            CRTC_INDEX => self.crtc_index = byte & 0x1f,
            CRTC_DATA => self.crtc[self.crtc_index as usize] = byte,
            _ => ()
        }
    }

    fn read(&mut self, port: u16) -> u8 {
        match port {
            AC_INDEX => self.ac_index,
            AC_READ => self.ac[(self.ac_index & 0x1f) as usize],
            SEQ_INDEX => self.seq_index,
            SEQ_DATA => self.seq[self.seq_index as usize],
            DAC_READ_INDEX => if self.dac_reading { DAC_STATE_READ } else { 0 },
            DAC_WRITE_INDEX => self.dac_write.0,
            DAC_DATA => {
                let (index, component) = self.dac_read;
                self.dac_read = if component == 2 { (index.wrapping_add(1), 0) } else { (index, component + 1) };
                self.palette[index as usize][component]
            },
            MISC_READ => self.misc,
            GC_INDEX => self.gc_index,
            GC_DATA => self.gc[self.gc_index as usize],
            CRTC_INDEX => self.crtc_index,
            CRTC_DATA => self.crtc[self.crtc_index as usize],
            INSTAT_READ => {
                self.ac_data_next = false;
                self.retrace = !self.retrace;
                if self.retrace { INSTAT_DD | INSTAT_VR } else { 0 }
            },
            _ => 0xff
        }
    }

    /* 0x00RRGGBB, each 6-bit component widened to 8 bits */
    fn palette_rgb(&self) -> [u32; 256] {
        let widen = |c: u8| ((c << 2) | (c >> 4)) as u32;
        self.palette.map(|[r, g, b]| widen(r) << 16 | widen(g) << 8 | widen(b))
    }
}

impl IODevice for Vga {
    /* A word access (outw to an index port sets index and data at once)
     * is a byte access to port and port + 1; a rep string access repeats
     * on the same port. */
    fn handle(&mut self, io: &IO, base: *mut u8) -> io::Result<()> {
        let size = io.size as usize;
        let data = unsafe { std::slice::from_raw_parts_mut(base.add(io.data_offset as usize), size * io.count as usize) };
        for access in data.chunks_mut(size) {
            for (i, byte) in access.iter_mut().enumerate() {
                let port = io.port + i as u16;
                match io.direction as u32 {
                    KVM_EXIT_IO_OUT => self.write(port, *byte),
                    KVM_EXIT_IO_IN => *byte = self.read(port),
                    _ => ()
                }
            }
        }
        Ok(())
    }
}

/* The frontends' threads lock it only to copy the palette, not while
 * reading the framebuffer. */
impl Display for Mutex<Vga> {
    fn size(&self) -> (u16, u16) { (WIDTH as u16, HEIGHT as u16) }

    fn render(&self, out: &mut [u32]) {
        let (palette, framebuffer) = {
            let vga = self.lock().unwrap();
            (vga.palette_rgb(), vga.framebuffer.filter(|_| vga.ac_index & AC_PAS != 0))
        };
        let Some(framebuffer) = framebuffer else {
            out.fill(0);
            return;
        };
        for (i, pixel) in out.iter_mut().enumerate().take(VGA_FRAMEBUFFER_SIZE) {
            *pixel = palette[read_guest_ptr::<u8>(unsafe { framebuffer.add(i) }) as usize];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode13h() {
        let mut fb = vec![0u8; VGA_FRAMEBUFFER_SIZE];
        fb[0] = 1;
        fb[VGA_FRAMEBUFFER_SIZE - 1] = 2;
        let vga = Vga::new(Some(fb.as_ptr()));
        let mut out = vec![0x123456u32; VGA_FRAMEBUFFER_SIZE];

        {
            let mut v = vga.lock().unwrap();
            v.write(DAC_WRITE_INDEX, 1);
            for c in [0x3f, 0x00, 0x20, 0x00, 0x3f, 0x01] {
                v.write(DAC_DATA, c);
            }
            v.write(DAC_READ_INDEX, 2);
            assert_eq!(v.read(DAC_READ_INDEX), DAC_STATE_READ);
            assert_eq!([v.read(DAC_DATA), v.read(DAC_DATA), v.read(DAC_DATA)], [0x00, 0x3f, 0x01]);
        }

        /* Blanked until the display is turned on */
        vga.render(&mut out);
        assert!(out.iter().all(|&p| p == 0));

        {
            let mut v = vga.lock().unwrap();
            v.read(INSTAT_READ);
            v.write(AC_INDEX, 0x10);
            v.write(AC_INDEX, 0x41);
            assert_eq!(v.read(AC_READ), 0x41);
            v.read(INSTAT_READ);
            v.write(AC_INDEX, AC_PAS);
        }
        vga.render(&mut out);
        assert_eq!(out[0], 0xff0082);
        assert_eq!(out[1], 0);
        assert_eq!(out[VGA_FRAMEBUFFER_SIZE - 1], 0x00ff04);
    }
}
