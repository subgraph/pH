extern crate libc;

mod error;
mod cmdline;
mod setup;
mod sys;

use crate::setup::Setup;

pub use error::{Error,Result};

fn run_init() -> Result<()> {
    Setup::check_pid1()?;
    let setup = Setup::create("airwolf")?;
    setup.set_splash(SPLASH);
    setup.setup_rootfs()?;
    setup.mount_home_if_exists()?;
    setup.launch_shell()?;
    Ok(())
}

fn main() {
    if let Err(err) = run_init() {
        println!("ph-init error: {}", err);
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