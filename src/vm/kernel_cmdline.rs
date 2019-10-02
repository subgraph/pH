use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;



fn add_defaults(cmdline: &mut KernelCmdLine) {
    cmdline
        .push("noapic")
        .push("noacpi")
        // keyboard reboot
        .push("reboot=k")
        .push_set_true("panic")

        .push("init_on_alloc=0")
        .push("init_on_free=0")
        .push_set_val("console", "hvc0")

        .push_set_true("i8042.direct")
        .push_set_true("i8042.dumbkbd")
        .push_set_true("i8042.nopnp")
        .push_set_true("i8042.noaux")
   //     .push("initcall_debug")
        .push_set_val("iommu", "off")
        .push("cryptomgr.notests")

        .push_set_val("8250.nr_uarts", "0");
}


pub struct KernelCmdLine {
    buffer: OsString,
}

impl KernelCmdLine {
    pub fn new() -> KernelCmdLine {
        KernelCmdLine { buffer: OsString::new() }
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

    pub fn size(&self) -> usize {
        (&self.buffer).as_bytes().len() + 1
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.buffer.as_bytes()
    }
}
