use crate::vm::{VmConfig, Result, Error, PHINIT, SOMMELIER};
use crate::vm::arch::ArchSetup;
use crate::vm::kernel_cmdline::KernelCmdLine;
use crate::vm::io::IoDispatcher;
use crate::devices;
use termios::Termios;
use crate::virtio::VirtioBus;
use crate::virtio;
use crate::devices::SyntheticFS;
use std::{fs, thread};
use crate::system::{Tap, NetlinkSocket};
use crate::disk::DiskImage;
use crate::kvm::{KvmVcpu, Kvm};
use std::sync::Arc;
use crate::memory::MemoryManager;
use std::sync::atomic::AtomicBool;
use crate::vm::run::KvmRunArea;

pub struct Vm {
    kvm: Kvm,
    vcpus: Vec<KvmVcpu>,
    memory: MemoryManager,
    io_dispatch: Arc<IoDispatcher>,
    termios: Option<Termios>,
}

impl Vm {
    fn create<A: ArchSetup>(arch: &mut A) -> Result<Self> {
        let kvm = arch.open_kvm()
            .map_err(Error::ArchError)?;
        let memory = arch.create_memory(&kvm)
            .map_err(Error::ArchError)?;
        Ok(Vm {
            kvm,
            memory,
            vcpus: Vec::new(),
            io_dispatch: IoDispatcher::new(),
            termios: None,
        })
    }

    pub fn start(&self) -> Result<()> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();
        for vcpu in self.vcpus.clone() {
            let mut run_area = KvmRunArea::new(vcpu, shutdown.clone(), self.io_dispatch.clone())?;
            let h = thread::spawn(move || run_area.run());
            handles.push(h);
        }

        for h in handles {
            h.join().expect("...");
        }
        if let Some(termios) = self.termios {
            let _ = termios::tcsetattr(0, termios::TCSANOW, &termios)
                .map_err(Error::TerminalTermios)?;
        }
        Ok(())

    }
}

pub struct VmSetup <T: ArchSetup> {
    config: VmConfig,
    cmdline: KernelCmdLine,
    arch: T,
}

impl <T: ArchSetup> VmSetup <T> {

    pub fn new(config: VmConfig, arch: T) -> Self {
        VmSetup {
            config,
            cmdline: KernelCmdLine::new_default(),
            arch,
        }
    }

    pub fn create_vm(&mut self) -> Result<Vm> {
        let mut vm = Vm::create(&mut self.arch)?;

        devices::rtc::Rtc::register(vm.io_dispatch.clone());

        if self.config.verbose() {
            self.cmdline.push("earlyprintk=serial");
            devices::serial::SerialDevice::register(vm.kvm.clone(),vm.io_dispatch.clone(), 0);
        } else {
            self.cmdline.push("quiet");
        }
        if self.config.rootshell() {
            self.cmdline.push("phinit.rootshell");
        }
        if vm.memory.drm_available() && self.config.is_dmabuf_enabled() {
            self.cmdline.push("phinit.virtwl_dmabuf");
        }

        if let Some(realm) = self.config.realm_name() {
            self.cmdline.push_set_val("phinit.realm", realm);
        }

        let saved= Termios::from_fd(0)
            .map_err(Error::TerminalTermios)?;
        vm.termios = Some(saved);

        let mut virtio = VirtioBus::new(vm.memory.clone(), vm.io_dispatch.clone(), vm.kvm.clone());
        self.setup_synthetic_bootfs(&mut virtio)?;
        self.setup_virtio(&mut virtio)
            .map_err(Error::SetupVirtio)?;

        if let Some(init_cmd) = self.config.get_init_cmdline() {
            self.cmdline.push_set_val("init", init_cmd);
        }

        self.arch.setup_memory(&self.cmdline, &virtio.pci_irqs())
            .map_err(Error::ArchError)?;

        for id in 0..self.config.ncpus() {
            let vcpu = vm.kvm.new_vcpu(id).map_err(Error::CreateVmFailed)?;
            self.arch.setup_vcpu(&vcpu).map_err(Error::ArchError)?;
            vm.vcpus.push(vcpu);
        }
        Ok(vm)
    }

    fn setup_virtio(&mut self, virtio: &mut VirtioBus) -> virtio::Result<()> {
        devices::VirtioSerial::create(virtio)?;
        devices::VirtioRandom::create(virtio)?;

        if self.config.is_wayland_enabled() {
            devices::VirtioWayland::create(virtio)?;
        }

        let homedir = self.config.homedir();
        devices::VirtioP9::create(virtio, "home", homedir, false, false)?;
        if homedir != "/home/user" && !self.config.is_realm() {
            self.cmdline.push_set_val("phinit.home", homedir);
        }

        let mut block_root = None;

        for disk in self.config.get_realmfs_images() {
            if block_root == None {
                block_root = Some(disk.read_only());
            }
            devices::VirtioBlock::create(virtio, disk)?;
        }

        for disk in self.config.get_raw_disk_images() {
            if block_root == None {
                block_root = Some(disk.read_only());
            }
            devices::VirtioBlock::create(virtio, disk)?;
        }

        if let Some(read_only) = block_root {
            if !read_only {
                self.cmdline.push("phinit.root_rw");
            }
            self.cmdline.push("phinit.root=/dev/vda");
            self.cmdline.push("phinit.rootfstype=ext4");
        } else {
            devices::VirtioP9::create(virtio, "9proot", "/", true, false)?;
            self.cmdline.push_set_val("phinit.root", "9proot");
            self.cmdline.push_set_val("phinit.rootfstype", "9p");
            self.cmdline.push_set_val("phinit.rootflags", "trans=virtio");
        }

        if self.config.network() {
            self.setup_network(virtio)?;
            self.drop_privs();

        }
        Ok(())
    }

    fn drop_privs(&self) {
        unsafe {
            libc::setgid(1000);
            libc::setuid(1000);
            libc::setegid(1000);
            libc::seteuid(1000);
        }

    }

    fn setup_synthetic_bootfs(&mut self, virtio: &mut VirtioBus) -> Result<()> {
        let bootfs = self.create_bootfs()
            .map_err(Error::SetupBootFs)?;

        devices::VirtioP9::create_with_filesystem(bootfs, virtio, "/dev/root", "/", false)
            .map_err(Error::SetupVirtio)?;

        self.cmdline.push_set_val("init", "/usr/bin/ph-init");
        self.cmdline.push_set_val("root", "/dev/root");
        self.cmdline.push("ro");
        self.cmdline.push_set_val("rootfstype", "9p");
        self.cmdline.push_set_val("rootflags", "trans=virtio");
        Ok(())
    }

    fn create_bootfs(&self) -> ::std::io::Result<SyntheticFS> {
        let mut s = SyntheticFS::new();
        s.mkdirs(&["/tmp", "/proc", "/sys", "/dev", "/home/user", "/bin", "/etc"]);

        fs::write("/tmp/ph-init", PHINIT)?;
        s.add_library_dependencies("/tmp/ph-init")?;
        fs::remove_file("/tmp/ph-init")?;

        s.add_memory_file("/usr/bin", "ph-init", 0o755, PHINIT)?;
        s.add_memory_file("/usr/bin", "sommelier", 0o755, SOMMELIER)?;

        s.add_file("/etc", "ld.so.cache", 0o644, "/etc/ld.so.cache");
        Ok(s)
    }

    fn setup_network(&mut self, virtio: &mut VirtioBus) -> virtio::Result<()> {
        let tap = match self.setup_tap() {
            Ok(tap) => tap,
            Err(e) => {
                warn!("failed to create tap device: {}", e);
                return Ok(());
            }
        };
        devices::VirtioNet::create(virtio, tap)?;
        self.cmdline.push("phinit.ip=172.17.0.22");
        Ok(())
    }

    fn setup_tap(&self) -> Result<Tap> {
        let bridge_name = self.config.bridge();
        let tap = Tap::new_default()?;
        let nl = NetlinkSocket::open()?;

        if !nl.interface_exists(bridge_name) {
            nl.create_bridge(bridge_name)?;
            nl.set_interface_up(bridge_name)?;
        }
        nl.add_interface_to_bridge(tap.name(), bridge_name)?;
        nl.set_interface_up(tap.name())?;
        Ok(tap)
    }
}