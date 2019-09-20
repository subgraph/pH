use std::{thread, fs};

use self::io::IoDispatcher;

use crate::virtio::VirtioBus;
use crate::devices;

use crate::memory::{GuestRam, KVM_KERNEL_LOAD_ADDRESS, MemoryManager, SystemAllocator, AddressRange};
use crate::kvm::*;

static KERNEL: &[u8] = include_bytes!("../../kernel/ph_linux");
static PHINIT: &[u8] = include_bytes!("../../ph-init/target/release/ph-init");
static SOMMELIER: &[u8] = include_bytes!("../../sommelier/sommelier");

mod run;
pub mod io;
mod setup;
mod error;
mod kernel_cmdline;
mod config;
pub use config::VmConfig;

pub use self::error::{Result,Error,ErrorKind};


use self::run::KvmRunArea;

use self::kernel_cmdline::KernelCmdLine;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use termios::Termios;
use crate::devices::SyntheticFS;

pub struct Vm {
    _config: VmConfig,
    memory: MemoryManager,
    io_dispatcher: Arc<IoDispatcher>,
    termios: Option<Termios>,
    _virtio: Arc<VirtioBus>,
}

static REQUIRED_EXTENSIONS: &[u32] = &[
    KVM_CAP_IRQCHIP,
    KVM_CAP_HLT,
    KVM_CAP_USER_MEMORY,
    KVM_CAP_SET_TSS_ADDR,
    KVM_CAP_EXT_CPUID,
    KVM_CAP_IRQ_ROUTING,
    KVM_CAP_IRQ_INJECT_STATUS,
    KVM_CAP_PIT2,
    KVM_CAP_IOEVENTFD,
];

fn get_base_dev_pfn(mem_size: u64) -> u64 {
    // Put device memory at a 2MB boundary after physical memory or 4gb, whichever is greater.
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    let mem_size_round_2mb = (mem_size + 2 * MB - 1) / (2 * MB) * (2 * MB);
    std::cmp::max(mem_size_round_2mb, 4 * GB) / 4096
}

impl Vm {
    fn create_kvm() -> Result<Kvm> {
        let kvm = Kvm::open(&REQUIRED_EXTENSIONS)?;
        kvm.set_tss_addr(0xFFFbd000)?;
        kvm.create_pit2()?;
        kvm.create_irqchip()?;
        Ok(kvm)
    }
    fn create_memory_manager(ram_size: usize) -> Result<MemoryManager> {
        let kvm = Self::create_kvm()?;
        let ram = GuestRam::new(ram_size, &kvm)?;
        let dev_addr_start = get_base_dev_pfn(ram_size as u64) * 4096;
        let dev_addr_size = u64::max_value() - dev_addr_start;
        let allocator = SystemAllocator::new(AddressRange::new(dev_addr_start,dev_addr_size as usize));
        Ok(MemoryManager::new(kvm, ram, allocator))
    }

    fn setup_virtio(config: &mut VmConfig, cmdline: &mut KernelCmdLine, virtio: &mut VirtioBus) -> Result<()> {
        devices::VirtioSerial::create(virtio)?;
        devices::VirtioRandom::create(virtio)?;
        devices::VirtioWayland::create(virtio)?;
        devices::VirtioP9::create(virtio, "home", config.homedir(), false, false)?;

        let mut block_root = false;

        for mut disk in config.get_realmfs_images() {
            disk.open().map_err(ErrorKind::DiskImageOpen)?;
            devices::VirtioBlock::create(virtio, disk)?;
            block_root = true;
        }
        for mut disk in config.get_raw_disk_images() {
            disk.open().map_err(ErrorKind::DiskImageOpen)?;
            devices::VirtioBlock::create(virtio, disk)?;
            block_root = true;
        }

        if block_root {
            cmdline.push("phinit.root=/dev/vda");
            cmdline.push("phinit.rootfstype=ext4");
        } else {
            devices::VirtioP9::create(virtio, "9proot", "/", true, false)?;
            cmdline.push_set_val("phinit.root", "9proot");
            cmdline.push_set_val("phinit.rootfstype", "9p");
            cmdline.push_set_val("phinit.rootflags", "trans=virtio");
        }

        Self::setup_synthetic_bootfs(cmdline, virtio)
    }

    fn setup_synthetic_bootfs(cmdline: &mut KernelCmdLine, virtio: &mut VirtioBus) -> Result<()> {
        let mut s = SyntheticFS::new();
        s.mkdirs(&["/tmp", "/proc", "/sys", "/dev", "/home/user", "/bin", "/etc"]);

        fs::write("/tmp/ph-init", PHINIT)?;
        s.add_library_dependencies("/tmp/ph-init")?;
        fs::remove_file("/tmp/ph-init")?;

        s.add_memory_file("/usr/bin", "ph-init", 0o755, PHINIT)?;
        s.add_memory_file("/usr/bin", "sommelier", 0o755, SOMMELIER)?;

        s.add_file("/etc", "ld.so.cache", 0o644, "/etc/ld.so.cache");
        devices::VirtioP9::create_with_filesystem(s, virtio, "/dev/root", "/", false)?;
        cmdline.push_set_val("init", "/usr/bin/ph-init");
        cmdline.push_set_val("root", "/dev/root");
        cmdline.push("ro");
        cmdline.push_set_val("rootfstype", "9p");
        cmdline.push_set_val("rootflags", "trans=virtio");
        Ok(())
    }

    pub fn open(mut config: VmConfig) -> Result<Vm> {

        let mut memory = Self::create_memory_manager(config.ram_size())?;

        let mut cmdline = KernelCmdLine::new_default();

        setup::kernel::load_pm_kernel(memory.guest_ram(), cmdline.address(), cmdline.size())?;

        let io_dispatch = IoDispatcher::new();

        memory.kvm_mut().create_vcpus(config.ncpus())?;

        devices::rtc::Rtc::register(io_dispatch.clone());

        if config.verbose() {
            cmdline.push("earlyprintk=serial");
            devices::serial::SerialDevice::register(memory.kvm().clone(),io_dispatch.clone(), 0);
        } else {
            cmdline.push("quiet");
        }
        if config.rootshell() {
            cmdline.push("phinit.rootshell");
        }
        if let Some(realm) = config.realm_name() {
            cmdline.push_set_val("phinit.realm", realm);
        }

        let saved= Termios::from_fd(0)
            .map_err(ErrorKind::TerminalTermios)?;
        let termios = Some(saved);

        let mut virtio = VirtioBus::new(memory.clone(), io_dispatch.clone(), memory.kvm().clone());
        Self::setup_virtio(&mut config, &mut cmdline, &mut virtio)?;

        if config.launch_systemd() {
            cmdline.push("phinit.run_systemd");
        }
        if let Some(init_cmd) = config.get_init_cmdline() {
            cmdline.push_set_val("init", init_cmd);
        }

        cmdline.write_to_memory(memory.guest_ram())?;

        setup::mptable::setup_mptable(memory.guest_ram(), config.ncpus(), virtio.pci_irqs())?;

        Ok(Vm {
            _config: config,
            memory,
            io_dispatcher: io_dispatch,
            termios,
            _virtio: Arc::new(virtio),
        })
    }

    pub fn start(&self) -> Result<()> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();
        for vcpu in self.memory.kvm().get_vcpus() {
            setup::cpu::setup_protected_mode(&vcpu, KVM_KERNEL_LOAD_ADDRESS + 0x200, self.memory.guest_ram())?;
            let mut run_area = KvmRunArea::new(vcpu, shutdown.clone(), self.io_dispatcher.clone())?;
            let h = thread::spawn(move || run_area.run());
            handles.push(h);
        }

        for h in handles {
            h.join().expect("...");
        }
        if let Some(termios) = self.termios {
            let _ = termios::tcsetattr(0, termios::TCSANOW, &termios)
                .map_err(ErrorKind::TerminalTermios)?;
        }
        Ok(())
    }
}

