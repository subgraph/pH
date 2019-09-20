use std::path::{Path, PathBuf};
use std::env;
use std::io::Result;
use std::process::Command;

fn main() -> Result<()> {
    build_phinit()?;
    run_simple_make("sommelier")?;
    run_simple_make("kernel")?;
    Ok(())
}

fn build_phinit() -> Result<()> {
    run_command_in_directory(
        "ph-init",
        "cargo",
        &["build", "--release"]
    )?;
    run_command_in_directory(
        "ph-init",
        "strip",
        &["target/release/ph-init"]
    )?;
    Ok(())
}

fn run_simple_make<P: AsRef<Path>>(directory: P) -> Result<()> {
    run_command_in_directory(directory, "make", &[])
}

fn run_command_in_directory<P: AsRef<Path>>(directory: P, cmd: &str, args: &[&str]) -> Result<()> {
    let saved = push_directory(directory)?;
    Command::new(cmd).args(args).status()?;
    env::set_current_dir(saved)
}

fn push_directory<P: AsRef<Path>>(directory: P) -> Result<PathBuf> {
    let current = env::current_dir()?;
    env::set_current_dir(directory.as_ref())?;
    Ok(current)
}




