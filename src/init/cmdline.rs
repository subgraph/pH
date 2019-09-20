use std::collections::HashMap;
use std::fs;
use std::path::Path;
use crate::sys::mount_procfs;
use crate::error::{Error,Result};

pub struct CmdLine {
    vars: HashMap<String, Option<String>>,
}

impl CmdLine {
    pub fn load() -> Result<Self> {
        let proc_path = Path::new("/proc/cmdline");
        if !proc_path.exists() {
            mount_procfs()?;
        }

        let cmdline = fs::read_to_string("/proc/cmdline")
            .map_err(Error::KernelCmdLine)?;
        Ok(Self::parse(cmdline))
    }

    fn parse(line: String) -> Self {
        let mut vars = HashMap::new();

        for v in line.split_whitespace() {
            if let Some(eq) = v.find('=') {
                let (key, val) = v.split_at(eq);
                let val = val.trim_start_matches('=');
                vars.insert(key.to_string(), Some(val.to_string()));
            } else {
                vars.insert(v.to_string(), None);
            }
        }
        CmdLine{ vars }
    }

    pub fn has_var(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    pub fn lookup(&self, name: &str) -> Option<String> {
        if let Some(val) = self.vars.get(name) {
            val.as_ref().cloned()
        } else {
            None
        }
    }
}
