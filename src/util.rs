fn read_le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn read_le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn read_le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..(o + 8)].try_into().unwrap())
}

fn write_le16(b: &mut [u8], o: usize, val: u16) -> () {
    b[o..(o + 2)].copy_from_slice(&val.to_le_bytes());
}

fn write_le32(b: &mut [u8], o: usize, val: u32) -> () {
    b[o..(o + 4)].copy_from_slice(&val.to_le_bytes());
}

fn read_string(b: &[u8], o: usize) -> io::Result<String> {
    let end = b[o..]
        .iter()
        .position(|&c| c == 0).ok_or(io::Error::other("Could not extract string."))?;
    let res = std::str::from_utf8(&b[o..(o + end)])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        .to_string();
    Ok(res)
}

