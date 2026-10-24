/*
 * RFB ("VNC") server, RFC 6143: shows a Display to any VNC client and
 * passes the client's keys to a Keyboard. No authentication or encryption, so
 * listen on localhost and tunnel over SSH, or on a private network.
 *
 * Every client gets two threads: one parses its messages, the other
 * sends framebuffer updates when the client has asked for one and the
 * frame has changed, polling the Display at UPDATE_INTERVAL. Updates
 * use the Raw encoding and cover the rows that changed.
 */

use std::io::{ self, Read, Write };
use std::net::{ TcpListener, TcpStream, ToSocketAddrs };
use std::sync::{ Arc, Mutex };
use std::sync::mpsc::{ self, Receiver, RecvTimeoutError, Sender };
use std::time::Duration;

const UPDATE_INTERVAL: Duration = Duration::from_millis(1000 / 60);
const DESKTOP_NAME: &[u8] = b"KVM Rust";
/* Shown, in black, while the VM has no display device. */
const NO_DISPLAY_SIZE: (u16, u16) = (320, 200);

const SECURITY_NONE: u8 = 1;
const ENCODING_RAW: i32 = 0;

const MSG_SET_PIXEL_FORMAT: u8 = 0;
const MSG_SET_ENCODINGS: u8 = 2;
const MSG_FRAMEBUFFER_UPDATE_REQUEST: u8 = 3;
const MSG_KEY_EVENT: u8 = 4;
const MSG_POINTER_EVENT: u8 = 5;
const MSG_CLIENT_CUT_TEXT: u8 = 6;
const MSG_FRAMEBUFFER_UPDATE: u8 = 0;

/* What a VNC client sees, e.g. a VGA adapter. */
pub trait Display: Send + Sync {
    /* Width and height in pixels; read once per client. */
    fn size(&self) -> (u16, u16);
    /* Fills `out` (width * height) with the current frame as 0x00RRGGBB. */
    fn render(&self, out: &mut [u32]);
}

/* What a VNC client types into, e.g. a PS/2 keyboard. */
pub trait Keyboard: Send {
    /* An X11 keysym went down or up. */
    fn key_event(&mut self, down: bool, keysym: u32) -> io::Result<()>;
}

struct NoDisplay;

impl Display for NoDisplay {
    fn size(&self) -> (u16, u16) { NO_DISPLAY_SIZE }
    fn render(&self, out: &mut [u32]) { out.fill(0); }
}

#[derive(Clone, Copy)]
struct PixelFormat {
    bits_per_pixel: u8,
    depth: u8,
    big_endian: bool,
    true_colour: bool,
    max: [u16; 3],
    shift: [u8; 3]
}

impl PixelFormat {
    /* What the server offers: 32-bit 0x00RRGGBB, little-endian. */
    const DEFAULT: Self = Self {
        bits_per_pixel: 32, depth: 24, big_endian: false, true_colour: true,
        max: [255, 255, 255], shift: [16, 8, 0]
    };

    fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            bits_per_pixel: b[0],
            depth: b[1],
            big_endian: b[2] != 0,
            true_colour: b[3] != 0,
            max: [u16::from_be_bytes([b[4], b[5]]), u16::from_be_bytes([b[6], b[7]]),
                  u16::from_be_bytes([b[8], b[9]])],
            shift: [b[10], b[11], b[12]]
        }
    }

    fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0] = self.bits_per_pixel;
        b[1] = self.depth;
        b[2] = self.big_endian as u8;
        b[3] = self.true_colour as u8;
        for i in 0..3 {
            b[4 + 2 * i..6 + 2 * i].copy_from_slice(&self.max[i].to_be_bytes());
        }
        b[10..13].copy_from_slice(&self.shift);
        b
    }

    fn bytes_per_pixel(&self) -> usize {
        self.bits_per_pixel as usize / 8
    }

    /* Colour-map formats would need SetColourMapEntries; every common
     * client asks for true colour. */
    fn supported(&self) -> bool {
        self.true_colour && matches!(self.bits_per_pixel, 8 | 16 | 32)
    }

    fn encode(&self, rgb: u32, out: &mut Vec<u8>) {
        let mut value = 0u32;
        for (i, component) in [(rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff].into_iter().enumerate() {
            value |= (component * self.max[i] as u32 / 255) << self.shift[i];
        }
        let bytes = value.to_le_bytes();
        let n = self.bytes_per_pixel();
        if self.big_endian {
            out.extend(bytes[..n].iter().rev());
        } else {
            out.extend_from_slice(&bytes[..n]);
        }
    }
}

/* Client messages the update thread cares about. */
enum Request {
    PixelFormat(PixelFormat),
    Update { incremental: bool }
}

pub fn start_rfb(addr: impl ToSocketAddrs, display: Option<Arc<dyn Display>>,
                 keyboard: Option<Arc<Mutex<dyn Keyboard>>>) -> io::Result<()> {
    let display = display.unwrap_or_else(|| Arc::new(NoDisplay));
    let listener = TcpListener::bind(addr)?;
    println!("RFB server on {}\r", listener.local_addr()?);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let display = display.clone();
            let keyboard = keyboard.clone();
            std::thread::spawn(move || {
                let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
                match serve(stream, display, keyboard) {
                    Ok(()) => println!("RFB client {} disconnected\r", peer),
                    Err(e) => println!("RFB client {}: {}\r", peer, e)
                }
            });
        }
    });
    Ok(())
}

fn serve(mut stream: TcpStream, display: Arc<dyn Display>,
         keyboard: Option<Arc<Mutex<dyn Keyboard>>>) -> io::Result<()> {
    stream.set_nodelay(true)?;
    handshake(&mut stream)?;

    let mut shared = [0u8; 1];
    stream.read_exact(&mut shared)?;
    let (width, height) = display.size();
    let mut init = Vec::with_capacity(24 + DESKTOP_NAME.len());
    init.extend_from_slice(&width.to_be_bytes());
    init.extend_from_slice(&height.to_be_bytes());
    init.extend_from_slice(&PixelFormat::DEFAULT.to_bytes());
    init.extend_from_slice(&(DESKTOP_NAME.len() as u32).to_be_bytes());
    init.extend_from_slice(DESKTOP_NAME);
    stream.write_all(&init)?;

    let (tx, rx) = mpsc::channel();
    let writer = stream.try_clone()?;
    let updates = std::thread::spawn(move || send_updates(writer, display, width, height, rx));

    let res = read_messages(&mut stream, keyboard.as_deref(), tx);
    /* Unblocks the update thread if it is the one still running. */
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = updates.join();
    match res {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
        res => res
    }
}

/* Versions 3.3, 3.7 and 3.8 differ only in how security is agreed on. */
fn handshake(stream: &mut TcpStream) -> io::Result<()> {
    stream.write_all(b"RFB 003.008\n")?;
    let mut version = [0u8; 12];
    stream.read_exact(&mut version)?;
    let minor = match &version {
        b"RFB 003.003\n" => 3,
        b"RFB 003.007\n" => 7,
        /* Later versions must be answered as 3.8. */
        v if v.starts_with(b"RFB 003.") => 8,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "not an RFB client"))
    };

    if minor == 3 {
        /* The server picks the security type. */
        return stream.write_all(&(SECURITY_NONE as u32).to_be_bytes());
    }
    stream.write_all(&[1, SECURITY_NONE])?;
    let mut chosen = [0u8; 1];
    stream.read_exact(&mut chosen)?;
    if chosen[0] != SECURITY_NONE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "client chose an unoffered security type"));
    }
    if minor == 8 {
        /* SecurityResult: OK */
        stream.write_all(&0u32.to_be_bytes())?;
    }
    Ok(())
}

fn read_messages(stream: &mut TcpStream, keyboard: Option<&Mutex<dyn Keyboard>>,
                 tx: Sender<Request>) -> io::Result<()> {
    let invalid = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_string());
    loop {
        let mut kind = [0u8; 1];
        stream.read_exact(&mut kind)?;
        let request = match kind[0] {
            MSG_SET_PIXEL_FORMAT => {
                let mut b = [0u8; 19];
                stream.read_exact(&mut b)?;
                let format = PixelFormat::from_bytes(b[3..].try_into().unwrap());
                if !format.supported() {
                    return Err(invalid("unsupported pixel format (colour map or odd size)"));
                }
                Some(Request::PixelFormat(format))
            },
            MSG_SET_ENCODINGS => {
                /* Raw is always allowed, and it is the only one sent. */
                let mut b = [0u8; 3];
                stream.read_exact(&mut b)?;
                let count = u16::from_be_bytes([b[1], b[2]]) as usize;
                let mut encodings = vec![0u8; 4 * count];
                stream.read_exact(&mut encodings)?;
                None
            },
            MSG_FRAMEBUFFER_UPDATE_REQUEST => {
                /* Every request is answered with the whole frame's changed
                 * rows; x, y, width and height are not looked at. */
                let mut b = [0u8; 9];
                stream.read_exact(&mut b)?;
                Some(Request::Update { incremental: b[0] != 0 })
            },
            MSG_KEY_EVENT => {
                let mut b = [0u8; 7];
                stream.read_exact(&mut b)?;
                if let Some(keyboard) = keyboard {
                    keyboard.lock().unwrap().key_event(b[0] != 0, u32::from_be_bytes([b[3], b[4], b[5], b[6]]))?;
                }
                None
            },
            MSG_POINTER_EVENT => {
                let mut b = [0u8; 5];
                stream.read_exact(&mut b)?;
                None
            },
            MSG_CLIENT_CUT_TEXT => {
                let mut b = [0u8; 7];
                stream.read_exact(&mut b)?;
                let len = u32::from_be_bytes([b[3], b[4], b[5], b[6]]) as u64;
                io::copy(&mut (&mut *stream).take(len), &mut io::sink())?;
                None
            },
            other => return Err(invalid(&format!("unknown message type {}", other)))
        };
        if let Some(request) = request {
            if tx.send(request).is_err() {
                return Ok(());
            }
        }
    }
}

fn send_updates(mut stream: TcpStream, display: Arc<dyn Display>, width: u16, height: u16,
                rx: Receiver<Request>) -> io::Result<()> {
    let (w, h) = (width as usize, height as usize);
    let mut format = PixelFormat::DEFAULT;
    let mut frame = vec![0u32; w * h];
    /* What the client has: None until the first update. */
    let mut sent: Option<Vec<u32>> = None;
    let mut pending = false;
    let mut out = Vec::new();

    loop {
        match rx.recv_timeout(UPDATE_INTERVAL) {
            Ok(Request::PixelFormat(f)) => {
                format = f;
                /* Pixels already sent were in the old format. */
                sent = None;
                continue;
            },
            Ok(Request::Update { incremental }) => {
                pending = true;
                if !incremental {
                    sent = None;
                }
            },
            Err(RecvTimeoutError::Timeout) => (),
            Err(RecvTimeoutError::Disconnected) => return Ok(())
        }
        if !pending {
            continue;
        }

        display.render(&mut frame);
        let rows = match &sent {
            None => Some((0, h)),
            Some(old) => changed_rows(old, &frame, w)
        };
        /* An incremental request waits until something changes. */
        let Some((first, last)) = rows else { continue };

        out.clear();
        out.extend_from_slice(&[MSG_FRAMEBUFFER_UPDATE, 0]);
        out.extend_from_slice(&1u16.to_be_bytes());
        for v in [0, first as u16, width, (last - first) as u16] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&ENCODING_RAW.to_be_bytes());
        for &pixel in &frame[first * w..last * w] {
            format.encode(pixel, &mut out);
        }
        stream.write_all(&out)?;

        match &mut sent {
            Some(old) => old.copy_from_slice(&frame),
            None => sent = Some(frame.clone())
        }
        pending = false;
    }
}

/* The first changed row and one past the last, or None. */
fn changed_rows(old: &[u32], new: &[u32], width: usize) -> Option<(usize, usize)> {
    let differs = |y: &usize| old[y * width..(y + 1) * width] != new[y * width..(y + 1) * width];
    let rows = old.len() / width;
    let first = (0..rows).find(differs)?;
    let last = (0..rows).rev().find(differs).unwrap();
    Some((first, last + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /* 4x2: red, green, blue, white over four greys; the first pixel
     * turns black after the first update. */
    struct Pattern {
        frame: Mutex<Vec<u32>>
    }

    impl Display for Pattern {
        fn size(&self) -> (u16, u16) { (4, 2) }
        fn render(&self, out: &mut [u32]) { out.copy_from_slice(&self.frame.lock().unwrap()); }
    }

    struct Keys(Vec<(bool, u32)>);

    impl Keyboard for Keys {
        fn key_event(&mut self, down: bool, keysym: u32) -> io::Result<()> {
            self.0.push((down, keysym));
            Ok(())
        }
    }

    fn read_update(client: &mut TcpStream, bytes_per_pixel: usize) -> ([u16; 4], Vec<u8>) {
        let mut header = [0u8; 16];
        client.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], &[MSG_FRAMEBUFFER_UPDATE, 0, 0, 1]);
        let rect = [0, 1, 2, 3].map(|i| u16::from_be_bytes([header[4 + 2 * i], header[5 + 2 * i]]));
        assert_eq!(&header[12..], &ENCODING_RAW.to_be_bytes());
        let mut pixels = vec![0u8; rect[2] as usize * rect[3] as usize * bytes_per_pixel];
        client.read_exact(&mut pixels).unwrap();
        (rect, pixels)
    }

    #[test]
    fn session() {
        let display = Arc::new(Pattern {
            frame: Mutex::new(vec![0xff0000, 0x00ff00, 0x0000ff, 0xffffff,
                                   0x101010, 0x202020, 0x303030, 0x404040])
        });
        let keys = Arc::new(Mutex::new(Keys(vec![])));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        start_rfb(addr, Some(display.clone()), Some(keys.clone())).unwrap();
        let mut client = TcpStream::connect(addr).unwrap();

        let mut version = [0u8; 12];
        client.read_exact(&mut version).unwrap();
        assert_eq!(&version, b"RFB 003.008\n");
        client.write_all(b"RFB 003.008\n").unwrap();
        let mut security = [0u8; 2];
        client.read_exact(&mut security).unwrap();
        assert_eq!(security, [1, SECURITY_NONE]);
        client.write_all(&[SECURITY_NONE]).unwrap();
        let mut result = [0u8; 4];
        client.read_exact(&mut result).unwrap();
        assert_eq!(result, [0; 4]);

        client.write_all(&[1]).unwrap();
        let mut init = [0u8; 24];
        client.read_exact(&mut init).unwrap();
        assert_eq!(&init[..4], &[0, 4, 0, 2]);
        assert_eq!(&init[4..20], &PixelFormat::DEFAULT.to_bytes());
        let mut name = vec![0u8; u32::from_be_bytes(init[20..24].try_into().unwrap()) as usize];
        client.read_exact(&mut name).unwrap();
        assert_eq!(name, DESKTOP_NAME);

        /* Full update in the default format: 0x00RRGGBB little-endian. */
        client.write_all(&[MSG_SET_ENCODINGS, 0, 0, 1, 0, 0, 0, 0]).unwrap();
        client.write_all(&[MSG_FRAMEBUFFER_UPDATE_REQUEST, 0, 0, 0, 0, 0, 0, 4, 0, 2]).unwrap();
        let (rect, pixels) = read_update(&mut client, 4);
        assert_eq!(rect, [0, 0, 4, 2]);
        assert_eq!(&pixels[..8], &[0, 0, 0xff, 0, 0, 0xff, 0, 0]);

        /* RGB565 big-endian; only row 0 changed. */
        let mut format = PixelFormat::DEFAULT;
        format.bits_per_pixel = 16;
        format.depth = 16;
        format.big_endian = true;
        format.max = [31, 63, 31];
        format.shift = [11, 5, 0];
        let mut msg = vec![MSG_SET_PIXEL_FORMAT, 0, 0, 0];
        msg.extend_from_slice(&format.to_bytes());
        client.write_all(&msg).unwrap();
        client.write_all(&[MSG_FRAMEBUFFER_UPDATE_REQUEST, 0, 0, 0, 0, 0, 0, 4, 0, 2]).unwrap();
        let (rect, pixels) = read_update(&mut client, 2);
        assert_eq!(rect, [0, 0, 4, 2]);
        assert_eq!(&pixels[..8], &[0xf8, 0x00, 0x07, 0xe0, 0x00, 0x1f, 0xff, 0xff]);

        display.frame.lock().unwrap()[0] = 0;
        client.write_all(&[MSG_FRAMEBUFFER_UPDATE_REQUEST, 1, 0, 0, 0, 0, 0, 4, 0, 2]).unwrap();
        let (rect, pixels) = read_update(&mut client, 2);
        assert_eq!(rect, [0, 0, 4, 1]);
        assert_eq!(&pixels[..2], &[0, 0]);

        /* 'a' down and up */
        client.write_all(&[MSG_KEY_EVENT, 1, 0, 0, 0, 0, 0, 0x61]).unwrap();
        client.write_all(&[MSG_KEY_EVENT, 0, 0, 0, 0, 0, 0, 0x61]).unwrap();
        client.write_all(&[MSG_FRAMEBUFFER_UPDATE_REQUEST, 0, 0, 0, 0, 0, 0, 4, 0, 2]).unwrap();
        read_update(&mut client, 2);
        assert_eq!(keys.lock().unwrap().0, vec![(true, 0x61), (false, 0x61)]);
    }
}
