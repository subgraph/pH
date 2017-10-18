extern crate libc;

use std::env;
use std::io;
use std::process::{self, Child,Command,Stdio};

mod sys;

use sys::*;

fn setup_overlay() -> io::Result<()> {

    // Just using /tmp temporarily as a path expected to exist
    // pivot_root() call will move this tmpfs mount
    mount_tmpfs("/tmp")?;
    mkdir("/tmp/ro")?;
    pivot_root("/tmp", "/tmp/ro")?;

    // Current layout:
    //
    //    /ro real root fs is now mounted here
    //    /   tmpfs has been swapped as / by pivot_root()
    //
    mkdir("/rw")?;
    mount_tmpfs("/rw")?;
    mkdir("/rw/upper")?;
    mkdir("/rw/work")?;

    //  Add this to current layout:
    //
    //    /rw        2nd tmpfs mounted here
    //    /rw/upper  empty dir on 2nd tmpfs
    //    /rw/work   empty dir on 2nd tmpfs

    mkdir("/overlay")?;
    mount_overlay("/overlay", "lowerdir=/ro,upperdir=/rw/upper,workdir=/rw/work")?;
    mkdir("/overlay/ro")?;
    mkdir("/overlay/rw")?;
    mkdir("/overlay/old-root")?;

    // And this:
    //
    //    /overlay overlay fs mounted here
    //    /overlay/ro       empty dir
    //    /overlay/rw       empty dir
    //    /overlay/old-root empty dir

    // About to pivot_root() to make /overlay new root fs.
    // Move /ro and /rw to expected post-pivot location.
    move_mount("/ro", "/overlay/ro")?;
    move_mount("/rw", "/overlay/rw")?;

    // swap in overlay as rootfs
    pivot_root("/overlay", "/overlay/old-root")?;

    // finally throw away 1st tmpfs
    umount("/old-root")?;
    rmdir("/old-root")?;
    Ok(())
}

fn setup_mounts() -> io::Result<()> {
    mount_sysfs("/sys")?;
    mount_procfs("/proc")?;
    mount_devtmpfs("/dev")?;
    mkdir("/dev/pts")?;
    mount_devpts("/dev/pts")?;
    Ok(())
}

fn setup() -> io::Result<()> {
    setup_overlay()?;
    setup_mounts()?;
    sethostname("Airwolf")?;
    setsid()?;
    set_controlling_tty(0, true)?;
    Ok(())
}

fn run_shell() -> io::Result<Child>{
    Command::new("/bin/bash")
        .env_clear()
        .env("TERM", "xterm-256color")
        .env("HOME", "/home/user")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

fn handle_waitpid_err(err: io::Error) -> ! {
    if let Some(errno) = err.raw_os_error() {
        if errno == libc::ECHILD {
            if let Err(err) = reboot(libc::RB_AUTOBOOT) {
                println!("reboot() failed: {:?}", err);
                process::exit(-1);
            }
        }
    }
    println!("error on waitpid: {:?}", err);
    process::exit(-1);
}

fn wait_for_child() -> i32 {
    let r = waitpid(-1, 0);
    if let Ok(pid) = r {
        return pid;
    }
    handle_waitpid_err(r.err().unwrap());
}

fn main() {
    if let Err(err) = setup() {
        println!("Error on setup(): {:?}", err); return;
    }
    let _child = match run_shell() {
        Ok(child) => child,
        Err(err) => { println!("Error launching shell: {:?}", err); return; }
    };
    loop {
        let _ = wait_for_child();
    }
}
