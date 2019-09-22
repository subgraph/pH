use std::path::{Path, PathBuf};
use std::env;
use std::io::Result;
use std::process::Command;

fn main() -> Result<()> {
    build_phinit()?;
    run_simple_make("sommelier")?;
    run_simple_make("kernel")?;
    // Rerun build.rs upon making or pulling in new commits
    println!("cargo:rerun-if-changed=.git/refs/heads/master");
    Ok(())
}

fn build_phinit() -> Result<()> {
    let _dir = ChdirTo::path("ph-init");

    Command::new("cargo")
        .arg("build")
        .arg("--release")
        .status()?;

    Command::new("strip")
        .arg("target/release/ph-init")
        .status()?;

    Ok(())
}

fn run_simple_make<P: AsRef<Path>>(directory: P) -> Result<()> {
    let _dir = ChdirTo::path(directory);
    Command::new("make").status()?;
    Ok(())
}

struct ChdirTo {
    saved: PathBuf,
}

impl ChdirTo {
    fn path<P: AsRef<Path>>(directory: P) -> ChdirTo {
        let saved = env::current_dir()
            .expect("current_dir()");
        env::set_current_dir(directory.as_ref())
            .expect("set_current_dir()");
        ChdirTo { saved }
    }
}

impl Drop for ChdirTo {
    fn drop(&mut self) {
        env::set_current_dir(&self.saved)
            .expect("restore current dir");
    }
}

