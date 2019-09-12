use std::path::{PathBuf, Path};
use crate::vm::{Vm, Result, ErrorKind};
use std::{env, process};

pub enum RootFS {
    SelfRoot,
    RealmFSImage(PathBuf),
    RawImage(PathBuf),
    RawOffset(PathBuf, usize),
}

pub struct VmConfig {
    ram_size: usize,
    ncpus: usize,
    verbose: bool,
    launch_systemd: bool,
    kernel_path: Option<PathBuf>,
    init_path: Option<PathBuf>,
    init_cmd: Option<String>,
    rootfs: RootFS,
}

#[allow(dead_code)]
impl VmConfig {
    pub fn new(args: env::Args) -> VmConfig {
        let mut config = VmConfig {
            ram_size: 256 * 1024 * 1024,
            ncpus: 1,
            verbose: false,
            launch_systemd: false,
            kernel_path: None,
            init_path: None,
            init_cmd: None,
            rootfs: RootFS::SelfRoot,
        };
        config.parse_args(args);
        config
    }

    pub fn ram_size_megs(mut self, megs: usize) -> Self {
        self.ram_size = megs * 1024 * 1024;
        self
    }

    pub fn num_cpus(mut self, ncpus: usize) -> Self {
        self.ncpus = ncpus;
        self
    }

    pub fn init_cmdline(mut self, val: &str) -> Self {
        self.init_cmd = Some(val.to_owned());
        self
    }

    pub fn kernel_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.kernel_path = Some(path.into());
        self
    }

    pub fn init_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.init_path = Some(path.into());
        self
    }

    pub fn use_realmfs<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.rootfs = RootFS::RealmFSImage(path.into());
        self
    }

    pub fn use_rawdisk<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.rootfs = RootFS::RawImage(path.into());
        self
    }

    pub fn use_rawdisk_with_offset<P: Into<PathBuf>>(mut self, path: P, offset: usize) -> Self {
        self.rootfs = RootFS::RawOffset(path.into(), offset);
        self
    }

    pub fn use_systemd(mut self) -> Self {
        self.launch_systemd = true;
        self
    }

    pub fn boot(self) {
        match Vm::open(self) {
            Ok(vm) => if let Err(err) = vm.start() {
                notify!("Error starting VM: {}", err);
            }
            Err(e) => notify!("Error creating VM: {}", e),
        }
    }

    pub fn ram_size(&self) -> usize {
        self.ram_size
    }

    pub fn ncpus(&self) -> usize {
        self.ncpus
    }

    pub fn verbose(&self) -> bool {
        self.verbose
    }

    pub fn launch_systemd(&self) -> bool {
        self.launch_systemd
    }

    pub fn get_kernel_path(&self) -> Result<PathBuf> {
        match self.kernel_path {
            Some(ref path) if path.exists() => return Ok(path.to_path_buf()),
            None => if let Some(path) = Self::search_kernel() {
                return Ok(path)
            }
            _ => {},
        }
        Err(ErrorKind::KernelNotFound.into())
    }

    pub fn get_init_path(&self) -> Result<PathBuf> {
        match self.init_path {
            Some(ref path) if path.exists() => return Ok(path.to_path_buf()),
            None => if let Some(path) = Self::search_init() {
                return Ok(path)
            }
            _ => {},
        }
        Err(ErrorKind::InitNotFound.into())
    }

    pub fn get_init_cmdline(&self) -> Option<&str> {
        self.init_cmd.as_ref().map(|s| s.as_str())
    }

    pub fn rootfs(&self) -> &RootFS {
        &self.rootfs
    }

    fn search_init() -> Option<PathBuf> {
        Self::search_binary("ph-init", &[
            "rust/target/release", "rust/target/debug",
            "target/debug", "target/release"
        ])
    }

    fn search_kernel() -> Option<PathBuf> {
        Self::search_binary("ph_linux", &["kernel", "../kernel"])
    }

    fn search_binary(name: &str, paths: &[&str]) -> Option<PathBuf> {
        let cwd = match env::current_dir() {
            Ok(cwd) => cwd,
            _ => return None,
        };

        for p in paths {
            let p = Path::new(p).join(name);
            let current = if p.is_absolute() {
                p
            } else {
                cwd.join(p)
            };
            if current.exists() {
                return Some(current);
            }
        }
        None
    }

    fn parse_args(&mut self, args: env::Args) {
        for arg in args.skip(1) {
            self.parse_one_arg(&arg);
        }
    }

    fn parse_one_arg(&mut self, arg: &str) {
        if arg == "-v" {
            self.verbose = true;
        } else {
            eprintln!("Unrecognized command line argument: {}", arg);
            process::exit(1);
        }
    }
}
