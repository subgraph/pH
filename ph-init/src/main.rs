#[macro_use]
extern crate lazy_static;

#[macro_use]
mod log;
mod error;
mod cmdline;
mod service;
mod init;
mod sys;

pub use error::{Error,Result};
pub use log::{Logger,LogLevel};
use crate::init::InitServer;

fn run_init() -> Result<()> {
    let mut server = InitServer::create("airwolf")?;
    server.setup_filesystem()?;
    server.run_daemons()?;
    server.launch_console_shell(SPLASH)?;
    server.run()?;
    Ok(())
}

fn main() {
    if let Err(err) = run_init() {
        warn!("ph-init error: {}", err);
    }
}

const SPLASH: &str = r#"
             ──────────────────────────────||───────────────────────────────
                                          [▭▭]
                                        /~~~~~~\
                                       │~~╲  ╱~~│
                                ≡≡][≡≡≡│___||___│≡≡≡][≡≡
                                 [::]  (   ()   )  [::]
                                        ~╱~~~~╲~
                                       ○'      `o
"#;