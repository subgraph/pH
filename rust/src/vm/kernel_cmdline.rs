use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use crate::memory::{GuestRam,KERNEL_CMDLINE_ADDRESS};
use super::Result;


fn add_defaults(cmdline: &mut KernelCmdLine) {
    cmdline
        .push("noapic")
        .push("noacpi")
        // keyboard reboot
        .push("reboot=k")
        .push_set_true("panic")
        .push_set_val("tsc", "reliable")
        .push("no_timer_check")
        // faster rcu updates
        .push_set_true("rcuupdate.rcu_expedited")
        // then restore to normal after booting
        .push_set_true("rcuupdate.rcu_normal_after_boot")
        .push_set_val("console", "hvc0")

        .push_set_true("i8042.direct")
        .push_set_true("i8042.dumbkbd")
        .push_set_true("i8042.nopnp")
        .push_set_true("i8042.noaux")
        .push("noreplace-smp")
        //.push("initcall_debug")
        .push_set_val("iommu", "off")
        .push("cryptomgr.notests")

        .push_set_val("8250.nr_uarts", "0");
}


pub struct KernelCmdLine {
    address: u64,
    buffer: OsString,
}

impl KernelCmdLine {
    pub fn new() -> KernelCmdLine {
        KernelCmdLine { address: KERNEL_CMDLINE_ADDRESS, buffer: OsString::new() }
    }

    pub fn new_default() -> KernelCmdLine {
        let mut cmdline = KernelCmdLine::new();
        add_defaults(&mut cmdline);
        cmdline
    }

    pub fn push(&mut self, option: &str) -> &mut Self {
        if !self.buffer.is_empty() {
            self.buffer.push(" ");
        }
        self.buffer.push(option);
        self
    }

    pub fn push_set_true(&mut self, flag_option: &str) -> &mut Self {
        self.push(&format!("{}=1", flag_option))
    }

    pub fn push_set_val(&mut self, var: &str, val: &str) -> &mut Self {
        self.push(&format!("{}={}", var, val))
    }

    pub fn address(&self) -> u64 {
        self.address
    }

    pub fn size(&self) -> usize {
        (&self.buffer).as_bytes().len() + 1
    }

    pub fn write_to_memory(&self, memory: &GuestRam) -> Result<()> {
        let bs = self.buffer.as_bytes();
        let len = bs.len();
        memory.write_bytes(KERNEL_CMDLINE_ADDRESS, bs)?;
        memory.write_int(KERNEL_CMDLINE_ADDRESS + len as u64, 0u8)?;
        Ok(())
    }
}
