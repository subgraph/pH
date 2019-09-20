use std::sync::Arc;

use crate::kvm::KvmVcpu;
use crate::memory::Mapping;
use super::Result;
use super::io::IoDispatcher;
use std::sync::atomic::{AtomicBool, Ordering};

const KVM_EXIT_UNKNOWN:u32 = 0;
const KVM_EXIT_IO:u32 = 2;
const KVM_EXIT_MMIO:u32 = 6;
const KVM_EXIT_INTR:u32 = 10;
const KVM_EXIT_SHUTDOWN:u32 = 8;
const KVM_EXIT_INTERNAL_ERROR: u32 = 17;
const KVM_EXIT_SYSTEM_EVENT:u32 = 24;

pub struct KvmRunArea {
    vcpu: KvmVcpu,
    io: Arc<IoDispatcher>,
    mapping: Mapping,
    shutdown: Arc<AtomicBool>,
}

pub struct IoExitData {
    dir_out: bool,
    size: usize,
    port: u16,
    count: usize,
    offset: usize,
}

pub struct MmioExitData {
    phys: u64,
    size: usize,
    write: bool,
}

impl KvmRunArea {
    pub fn new(vcpu: KvmVcpu, shutdown: Arc<AtomicBool>, io_dispatcher: Arc<IoDispatcher>) -> Result<KvmRunArea> {
        let size = vcpu.get_vcpu_mmap_size()?;
        let mapping = Mapping::new_from_fd(vcpu.raw_fd(), size)?;
        Ok(KvmRunArea{
            vcpu,
            io: io_dispatcher,
            mapping,
            shutdown,
        })
    }

    fn r8(&self, offset: usize) -> u8 { self.mapping.read_int(offset).unwrap() }
    fn r16(&self, offset: usize) -> u16 { self.mapping.read_int(offset).unwrap() }
    fn r32(&self, offset: usize) -> u32 { self.mapping.read_int(offset).unwrap() }
    fn r64(&self, offset: usize) -> u64 { self.mapping.read_int(offset).unwrap() }
    fn w8(&self, offset: usize, val: u8) { self.mapping.write_int(offset, val).unwrap() }
    fn w16(&self, offset: usize, val: u16) { self.mapping.write_int(offset, val).unwrap() }
    fn w32(&self, offset: usize, val: u32) { self.mapping.write_int(offset, val).unwrap() }
    fn w64(&self, offset: usize, val: u64) { self.mapping.write_int(offset, val).unwrap() }

    fn exit_reason(&self) -> u32 {
        self.r32(8)
    }

    fn suberror(&self) -> u32 {
        self.r32(32)
    }

    fn get_io_exit(&self) -> IoExitData {
        let d = self.r8(32) != 0;
        let size = self.r8(33) as usize;
        let port = self.r16(34);
        let count = self.r32(36) as usize;
        let offset = self.r64(40) as usize;

        IoExitData{
            dir_out: d,
            size,
            port,
            count,
            offset,
        }
    }

    fn get_mmio_exit(&self) -> MmioExitData {
        let phys = self.r64(32);
        let size = self.r32(48) as usize;
        assert!(size <= 8);
        let write = self.r8(52) != 0;
        MmioExitData {
            phys, size, write
        }
    }

    pub fn run(&mut self) {
        loop {
            if let Err(err) = self.vcpu.run() {
                if !err.is_interrupted() {
                    println!("KVM_RUN returned error, bailing: {:?}", err);
                    return;
                }
            } else {
               self.handle_exit();
            }
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }
        }
    }

    fn handle_exit(&mut self) {
        match self.exit_reason() {
            KVM_EXIT_UNKNOWN => {println!("unknown")},
            KVM_EXIT_IO => { self.handle_exit_io() },
            KVM_EXIT_MMIO => { self.handle_exit_mmio() },
            KVM_EXIT_INTR => { println!("intr")},
            KVM_EXIT_SHUTDOWN => {
                self.handle_shutdown();
            },
            KVM_EXIT_SYSTEM_EVENT => { println!("event")},
            KVM_EXIT_INTERNAL_ERROR => {
                let sub = self.suberror();
                println!("internal error: {}", sub);
                println!("{:?}", self.vcpu.get_regs().unwrap());
                println!("{:?}", self.vcpu.get_sregs().unwrap());
            }
            n => { println!("unhandled exit: {}", n);},
        }
    }

    fn handle_shutdown(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    fn _handle_problem(&mut self) {
        let regs = self.vcpu.get_regs().unwrap();
        let sregs = self.vcpu.get_sregs().unwrap();
        println!("REGS:\n{:?}", regs);
        println!("SREGS:\n{:?}", sregs);
        panic!(":(");

    }

    fn handle_exit_io(&mut self) {
        let exit = self.get_io_exit();
        if exit.dir_out {
            self.handle_exit_io_out(&exit);
        } else {
            self.handle_exit_io_in(&exit);
        }
    }

    fn handle_exit_io_in(&mut self, exit: &IoExitData) {
        for i in 0..exit.count {
            let v = self.io.emulate_io_in(exit.port, exit.size);
            match exit.size {
                1 => self.w8(exit.offset + i, v as u8),
                2 => self.w16(exit.offset + i * 2, v as u16),
                4 => self.w32(exit.offset + i * 4, v as u32),
                _ => {},
            }
        }
    }

    fn handle_exit_io_out(&self, exit: &IoExitData) {
        for i in 0..exit.count {
            let v = match exit.size {
                1 => self.r8(exit.offset + i) as u32,
                2 => self.r16(exit.offset + i * 2) as u32,
                4 => self.r32(exit.offset + i * 4) as u32,
                _ => 0,
            };
            self.io.emulate_io_out(exit.port, exit.size, v);
        }
    }

    fn handle_exit_mmio(&mut self) {
        let exit = self.get_mmio_exit();
        if exit.write {
            self.handle_mmio_write(exit.phys, exit.size)
        } else {
            self.handle_mmio_read(exit.phys, exit.size)
        }
    }

    fn handle_mmio_write(&self, address: u64, size: usize) {
        if let Some(val) = self.data_to_val64(size) {
            self.io.emulate_mmio_write(address, size, val)
        }
    }

    fn handle_mmio_read(&self, address: u64, size: usize)  {
        if size == 1 || size == 2 || size == 4 || size == 8 {
            let val = self.io.emulate_mmio_read(address, size);
            match size {
                1 => self.w8(40, val as u8),
                2 => self.w16(40, val as u16),
                4 => self.w32(40, val as u32),
                8 => self.w64(40, val),
                _ => (),
            }
        }
    }

    fn data_to_val64(&self, size: usize) -> Option<u64> {
        match size {
            1 => { Some(self.r8(40) as u64)}
            2 => { Some(self.r16(40) as u64)}
            4 => { Some(self.r32(40) as u64)}
            8 => { Some(self.r64(40))}
            _ => { None }
        }
    }
}

