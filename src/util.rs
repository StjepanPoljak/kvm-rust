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

fn mmap_mem_region(mem_size: usize) -> io::Result<*mut libc::c_void> {
    // 140900 mmap(NULL, 1075838976, 0 /* PROT_NONE */, 0x22 /* MAP_PRIVATE|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
    // 140900 mmap(0x7768b3e00000, 1073741824, 0x3 /* PROT_READ|PROT_WRITE */, 0x32 /* MAP_PRIVATE|MAP_FIXED|MAP_ANONYMOUS */, -1, 0) = 0x7768b3e00000
    let mem_ptr = unsafe {
	libc::mmap(ptr::null_mut(),
		   mem_size as usize,
		   libc::PROT_READ|libc::PROT_WRITE,
		   libc::MAP_PRIVATE|libc::MAP_ANONYMOUS,
		   -1,
		   0)
    };
    if mem_ptr == libc::MAP_FAILED {
	return Err(io::Error::last_os_error());
    }
    Ok(mem_ptr)
}
