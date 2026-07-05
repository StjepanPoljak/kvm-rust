include!(concat!(env!("OUT_DIR"), "/kvm-bindings.rs"));

use std::io::{self, Read};
use libc::{_IOW, _IO, _IOR};

use crate::KVMIO;
use crate::VCPU;

const KVM_GET_SREGS2 : u64 = _IOR::<kvm_sregs2>(KVMIO, 0xcc);
const KVM_SET_SREGS2 : u64 = _IOW::<kvm_sregs2>(KVMIO, 0xcd);
const KVM_GET_REGS : u64 = _IOR::<kvm_regs>(KVMIO, 0x81);
const KVM_SET_REGS : u64 = _IOW::<kvm_regs>(KVMIO, 0x82);

impl kvm_regs {
    pub fn print(&self) {
       println!("RAX={:#x}\tRBX={:#x}\tRCX={:#x}\tRDX={:#x}", self.rax, self.rbx, self.rcx, self.rdx);
       println!("RSI={:#x}\tRDI={:#x}\tRSP={:#x}\tRBP={:#x}", self.rsi, self.rdi, self.rsp, self.rbp);
       println!("R8={:#x}\tR9={:#x}\tR10={:#x}\tR11={:#x}", self.r8, self.r9, self.r10, self.r11);
       println!("R12={:#x}\tR13={:#x}\tR14={:#x}\tR15={:#x}", self.r12, self.r13, self.r14, self.r15);
       println!("RIP={:#x}\tRFLAGS={:#x}", self.rip, self.rflags);
    }
}

impl kvm_segment {
    pub fn print(&self, name: &str) {
        println!("{name}\tbase={:#x}\tselector={:#x}\tlimit={:#x}\ttype={:#x}\tpresent={:#x}",
                 self.base, self.selector, self.limit, self.type_, self.present);
        println!("\tdpl={:#x}\t\tdb={:#x}\t\ts={:#x}\tl={:#x}\tg={:#x}\t\tavl={:#x}\n",
                 self.dpl, self.db, self.s, self.l, self.g, self.avl);
    }
}

impl kvm_dtable {
    pub fn print(&self, name: &str) {
        println!("{name}\tbase={:#x}\tlimit={:#x}\n", self.base, self.limit);
    }
}

impl kvm_sregs2 {
    pub fn print(&self) {
        self.cs.print("CS");
        self.ds.print("DS");
        self.es.print("ES");
        self.fs.print("FS");
        self.gs.print("GS");
        self.ss.print("SS");
        self.tr.print("TR");
        self.ldt.print("LDT");
        self.gdt.print("GDT");
        self.idt.print("IDT");

        println!("CR0={:#x}\t\tCR2={:#x}\t\tCR3={:#x}\t\tCR4={:#x}\t\tCR8={:#x}\n",
                 self.cr0, self.cr2, self.cr3, self.cr4, self.cr8);
        println!("EFER={:#x}\t\tAPIC_BASE={:#x}\t\tFLAGS={:#x}\n",
                 self.efer, self.apic_base, self.flags);

        for i in 0..4 {
            print!("PDPTRS[{i}]={:#x}", self.pdptrs[i]);
            if i == 3 {
                println!("");
            } else {
                print!("\t\t");
            }
        }
    }
}

impl VCPU {

    pub fn get_sregs2(&mut self) -> io::Result<kvm_sregs2> {
        let mut sregs2 = unsafe { std::mem::zeroed() };
        // 140904 ioctl(10<anon_inode:kvm-vcpu:0>, 0x8140aecc /* KVM_GET_SREGS2 */, 0x77690198f310) = 0
        let ret = unsafe {
            libc::ioctl(self.fd, KVM_GET_SREGS2, &mut sregs2)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(sregs2)
    }

    pub fn set_sregs2(&self, sregs2: kvm_sregs2) -> io::Result<()> {
        let ret = unsafe {
            libc::ioctl(self.fd, KVM_SET_SREGS2, &sregs2)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn get_regs(&mut self) -> io::Result<kvm_regs> {
        let mut regs = unsafe { std::mem::zeroed() };
        // 140904 ioctl(10<anon_inode:kvm-vcpu:0>, 0x8090ae81 /* KVM_GET_REGS */, {rax=0, ..., rsp=0, rbp=0, ..., rip=0xfff0, rflags=0x2}) = 0
        let ret = unsafe {
            libc::ioctl(self.fd, KVM_GET_REGS, &mut regs)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(regs)
    }

    pub fn set_regs(&self, regs: kvm_regs) -> io::Result<()> {
        let ret = unsafe {
            libc::ioctl(self.fd, KVM_SET_REGS, &regs)
        };
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn set_ip(&mut self, ip: usize) -> io::Result<()> {
        let mut regs = self.get_regs()?;
        regs.rip = ip as u64;
        self.set_regs(regs)
    }

    pub fn get_ip(&mut self) -> io::Result<usize> {
        let rip = self.get_regs()?.rip;
        Ok(rip as usize)
    }

    pub fn print_regs(&mut self) -> io::Result<()> {
        let regs = self.get_regs()?;
        let sregs2 = self.get_sregs2()?;

        regs.print();
        sregs2.print();

        Ok(())
    }
}
