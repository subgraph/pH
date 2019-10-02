static KERNEL: &[u8] = include_bytes!("../../kernel/ph_linux");
static PHINIT: &[u8] = include_bytes!("../../ph-init/target/release/ph-init");
static SOMMELIER: &[u8] = include_bytes!("../../sommelier/sommelier");

pub mod arch;
mod run;
pub mod io;
mod setup;
mod error;
mod kernel_cmdline;
mod config;

pub use config::VmConfig;
pub use setup::VmSetup;

pub use self::error::{Result,Error};
pub use arch::{ArchSetup,create_setup};


