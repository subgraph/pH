use std::io;
use std::ptr;
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use crate::error::{Result,Error};

use libc;
use std::path::Path;


pub fn mount_tmpfs(target: &str) -> Result<()> {
    mount("tmpfs", target, "tmpfs", 0, Some("mode=755"))
        .map_err(|e| Error::MountTmpFS(target.to_string(), e))
}

pub fn mount_tmpdir(target: &str) -> Result<()> {
    mount("tmpfs", target, "tmpfs",
          libc::MS_NOSUID|libc::MS_NODEV,
          Some("mode=1777"))
        .map_err(|e| Error::MountTmpFS(target.to_string(), e))
}

pub fn mount_procfs() -> Result<()> {
    mount("proc", "/proc", "proc",
          libc::MS_NOATIME|libc::MS_NOSUID|libc::MS_NODEV|libc::MS_NOEXEC,
          None)
        .map_err(Error::MountProcFS)
}

pub fn mount_sysfs() -> Result<()> {
    mount("sysfs", "/sys", "sysfs",
          libc::MS_NOATIME|libc::MS_NOSUID|libc::MS_NODEV|libc::MS_NOEXEC,
          None)
        .map_err(Error::MountSysFS)
}

pub fn mount_cgroup() -> Result<()> {
    mount("cgroup", "/sys/fs/cgroup", "cgroup",
          libc::MS_NOSUID|libc::MS_NODEV|libc::MS_NOEXEC,
          None)
        .map_err(Error::MountCGroup)
}

pub fn mount_devtmpfs() -> Result<()> {
    mount("devtmpfs", "/dev", "devtmpfs",
          libc::MS_NOSUID|libc::MS_NOEXEC,
          None)
        .map_err(Error::MountDevTmpFS)
}

pub fn mount_devpts() -> Result<()> {
    let target = "/dev/pts";
    if !Path::new(target).exists() {
        mkdir(target)?;
    }
    mount("devpts", target, "devpts",
          libc::MS_NOSUID|libc::MS_NOEXEC,
          Some("mode=620"))
        .map_err(Error::MountDevPts)
}

pub fn mount_overlay(target: &str, args: &str) -> Result<()> {
    mount("overlay", target, "overlay", 0, Some(args))
        .map_err(Error::MountOverlay)
}

pub fn move_mount(source: &str, target: &str) -> Result<()> {
    mount(source, target, "", libc::MS_MOVE, None)
        .map_err(|e| Error::MoveMount(source.to_string(), target.to_string(), e))
}

pub fn mount_9p(name: &str, target: &str) -> Result<()> {
    const MS_LAZYTIME: libc::c_ulong = (1 << 25);
    mount(name, target, "9p",
          libc::MS_NOATIME|MS_LAZYTIME,
          Some("trans=virtio,cache=loose"))
        .map_err(|e| Error::Mount9P(name.to_string(), target.to_string(), e))
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}

pub fn create_directories<P: AsRef<Path>>(directories: &[P]) -> Result<()> {
    for dir in directories {
        mkdir(dir)?;
    }
    Ok(())
}

pub fn mkdir<P: AsRef<Path>>(path: P) -> Result<()> {
    mkdir_mode(path, 0o755)
}

pub fn mkdir_mode<P: AsRef<Path>>(path: P, mode: u32) -> Result<()> {
    let path = path.as_ref();
    let path_cstr = CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::CStringConv)?;

    unsafe {
        if libc::mkdir(path_cstr.as_ptr(), mode) == -1 {
            return Err(Error::MkDir(path.display().to_string(), io::Error::last_os_error()))
        }
    }
    Ok(())
}

pub fn sethostname<S: AsRef<OsStr>>(name: S) -> Result<()> {
    let ptr = name.as_ref().as_bytes().as_ptr() as *const libc::c_char;
    let len = name.as_ref().len() as libc::size_t;
    unsafe {
        if libc::sethostname(ptr, len) < 0 {
            let last = io::Error::last_os_error();
            return Err(Error::SetHostname(last))
        }
    }
    Ok(())
}

pub fn setsid() -> Result<u32> {
    _setsid().map_err(Error::SetSid)
}

pub fn _setsid() -> io::Result<u32> {
    unsafe {
        let res = libc::setsid();
        if res == -1 {
            return Err(io::Error::last_os_error())
        }
        Ok(res as u32)
    }
}

pub fn mount(source: &str, target: &str, fstype: &str, flags: u64, data: Option<&str>)
    -> io::Result<()> where {

    let source = cstr(source);
    let target = cstr(target);
    let fstype = cstr(fstype);

    let data = data.map(|s| cstr(s) );
    let data_ptr = match data {
        Some(ref s) => s.as_ptr(),
        None => ptr::null(),
    };

    unsafe {
        if libc::mount(source.as_ptr(),
                       target.as_ptr(),
                       fstype.as_ptr(),
                       flags,
                       data_ptr as *const libc::c_void) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn pivot_root(new_root: &str, put_old: &str) -> Result<()> {
    let _new_root = cstr(new_root);
    let _put_old = cstr(put_old);
    unsafe {
        if libc::syscall(libc::SYS_pivot_root, _new_root.as_ptr(), _put_old.as_ptr()) == -1 {
            let last = io::Error::last_os_error();
            return Err(Error::PivotRoot(new_root.to_string(), put_old.to_string(), last))
        }
    }
    Ok(())
}

pub fn umount(path: &str) -> Result<()> {
    let _path = cstr(path);
    unsafe {
        if libc::umount(_path.as_ptr()) == -1 {
            let last = io::Error::last_os_error();
            return Err(Error::Umount(path.to_string(), last))
        }
    }
    Ok(())
}

pub fn set_controlling_tty(fd: libc::c_int, force: bool) -> Result<()> {
    let flag: libc::c_int = if force { 1 } else { 0 };
    unsafe {
        if libc::ioctl(fd, libc::TIOCSCTTY, flag) == -1 {
            let last = io::Error::last_os_error();
            return Err(Error::SetControllingTty(last))
        }
        Ok(())
    }
}

pub fn waitpid(pid: libc::pid_t, options: libc::c_int) -> io::Result<(i32,i32)> {
    let mut status = 0 as libc::c_int;
    unsafe {
        let ret = libc::waitpid(pid, &mut status, options);
        if ret == -1 {
            return Err(io::Error::last_os_error())
        }
        Ok((ret, status))
    }
}

pub fn getpid() -> i32 {
    unsafe { libc::getpid() }
}

pub fn chmod(path: &str, mode: u32) -> Result<()> {
    let path = cstr(path);
    unsafe {
        if libc::chmod(path.as_ptr(), mode) == -1 {
            let last = io::Error::last_os_error();
            return Err(Error::ChmodFailed(last));
        }

    }
    Ok(())
}

pub fn chown(path: &str, uid: u32, gid: u32) -> Result<()> {
    let path = cstr(path);
    unsafe {
        if libc::chown(path.as_ptr(), uid, gid) == -1 {
            let last = io::Error::last_os_error();
            return Err(Error::ChmodFailed(last));
        }
    }
    Ok(())
}
pub fn reboot(cmd: libc::c_int) -> io::Result<()> {
    unsafe {
        if libc::reboot(cmd) == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}