use std::sync::Arc;
use std::thread;
use std::path::{PathBuf,Path};
use std::env;

use self::io::IoDispatcher;

use virtio::VirtioBus;
use devices;

use memory::{GuestRam,KVM_KERNEL_LOAD_ADDRESS};
use kvm::*;


mod run;
pub mod io;
mod setup;
mod error;
mod kernel_cmdline;

pub use self::error::{Result,Error,ErrorKind};


use self::run::KvmRunArea;

use self::kernel_cmdline::KernelCmdLine;

pub struct VmConfig {
    ram_size: usize,
    ncpus: usize,
    kernel_path: PathBuf,
    init_path: PathBuf,
}

#[allow(dead_code)]
impl VmConfig {
    pub fn new() -> VmConfig {
        VmConfig {
            ram_size: 256 * 1024 * 1024,
            ncpus: 1,
            kernel_path: PathBuf::new(),
            init_path: PathBuf::new(),
        }
    }

    pub fn ram_size_megs(&mut self, megs: usize) {
        self.ram_size = megs * 1024 * 1024;
    }

    pub fn num_cpus(&mut self, ncpus: usize) {
        self.ncpus = ncpus;
    }

    pub fn kernel_path(&mut self, path: &Path) {
        self.kernel_path = path.to_path_buf();
    }

    pub fn init_path(&mut self, path: &Path) {
        self.init_path = path.to_path_buf();
    }


}
pub struct Vm {
    kvm: Kvm,
    memory: GuestRam,
    io_dispatcher: Arc<IoDispatcher>,
    _virtio: VirtioBus,
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

impl Vm {
    pub fn open(config: VmConfig) -> Result<Vm> {
        let mut kvm = Kvm::open(&REQUIRED_EXTENSIONS)?;

        kvm.set_tss_addr(0xFFFbd000)?;
        kvm.create_pit2()?;

        let memory = GuestRam::new(config.ram_size, &kvm)?;

        kvm.create_irqchip()?;

        let verbose = env::args().any(|arg| arg == "-v");
        let cmdline = KernelCmdLine::new_default(verbose);

        cmdline.write_to_memory(&memory)?;
        let path = PathBuf::from(&config.kernel_path);
        setup::kernel::load_pm_kernel(&memory, &path, cmdline.address(), cmdline.size())?;

        let io_dispatch = IoDispatcher::new();

        kvm.create_vcpus(config.ncpus)?;

        devices::rtc::Rtc::register(io_dispatch.clone());

        if verbose {
            devices::serial::SerialDevice::register(kvm.clone(),io_dispatch.clone(), 0);
        }

        let mut virtio = VirtioBus::new(memory.clone(), io_dispatch.clone(), kvm.clone());
        devices::VirtioSerial::create(&mut virtio)?;
        devices::VirtioRandom::create(&mut virtio)?;
        devices::VirtioP9::create(&mut virtio, "/dev/root", "/", &config.init_path)?;

        setup::mptable::setup_mptable(&memory, config.ncpus, virtio.pci_irqs())?;

        Ok(Vm {
            kvm,
            memory,
            io_dispatcher: io_dispatch,
            _virtio: virtio,
        })
    }

    pub fn start(&self) -> Result<()> {
        let mut handles = Vec::new();
        for vcpu in self.kvm.get_vcpus() {
            setup::cpu::setup_protected_mode(&vcpu, KVM_KERNEL_LOAD_ADDRESS + 0x200, &self.memory)?;
            let mut run_area = KvmRunArea::new(vcpu,  self.io_dispatcher.clone())?;
            let h = thread::spawn(move || run_area.run());
            handles.push(h);
        }

        for h in handles {
            h.join().expect("...");
        }
        Ok(())
    }

}

