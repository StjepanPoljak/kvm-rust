use object::elf;
use object::read::elf::{ElfFile64, ProgramHeader};
use crate::VM;
use object::{Endianness};
use std::io;

pub fn load_elf(vm: &mut VM, elf: &ElfFile64<Endianness>, elf_data: &[u8], mem_idx: usize) -> io::Result<u64> {
    let endian = elf.endian();
    let mut last_addr = 0u64;
    for ph in elf.elf_program_headers() {
        if ph.p_type(endian) != elf::PT_LOAD {
            continue;
        }
        let elf_start_ofs = ph.p_offset(endian) as usize;
        let elf_end_ofs = elf_start_ofs + ph.p_filesz(endian) as usize;
        let guest_mem_ofs = ph.p_paddr(endian);
        /* use p_memsz so the segment's .bss is included */
        let guest_mem_end = guest_mem_ofs + ph.p_memsz(endian);
        if guest_mem_end > last_addr {
            last_addr = guest_mem_end;
        }
        vm.load_data_to_memory(mem_idx, elf_data[elf_start_ofs..elf_end_ofs].to_vec(), guest_mem_ofs)?;
    }

    Ok(last_addr)
}
