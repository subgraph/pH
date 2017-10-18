use std::io;
use std::ptr;
use std::ffi::{CString,OsStr};
use std::os::unix::ffi::OsStrExt;

use libc;


pub fn mount_tmpfs(target: &str) -> io::Result<()> {
    mount("tmpfs", target, "tmpfs", 0, Some("mode=755"))
}

pub fn mount_procfs(target: &str) -> io::Result<()> {
    mount("proc", target, "proc", 0, None)
}

pub fn mount_sysfs(target: &str) -> io::Result<()> {
    mount("sysfs", target, "sysfs", 0, None)
}

pub fn mount_devtmpfs(target: &str) -> io::Result<()> {
    mount("devtmpfs", target, "devtmpfs", 0, None)
}

pub fn mount_devpts(target: &str) -> io::Result<()> {
    mount("devpts", target, "devpts", 0, None)
}

pub fn mount_overlay(target: &str, args: &str) -> io::Result<()> {
    mount("overlay", target, "overlay", 0, Some(args))
}

pub fn move_mount(source: &str, target: &str) -> io::Result<()> {
    mount(source, target, "", libc::MS_MOVE, None)
}


fn cstr(s: &str) -> CString {
    CString::new(s).unwrap()
}


pub fn mkdir(path: &str) -> io::Result<()> {

    let path = cstr(path);
    unsafe {
        if libc::mkdir(path.as_ptr(), 0o755) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn rmdir(path: &str) -> io::Result<()> {
    let path = cstr(path);
    unsafe {
        if libc::rmdir(path.as_ptr()) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())

}

pub fn sethostname<S: AsRef<OsStr>>(name: S) -> io::Result<()> {
    let ptr = name.as_ref().as_bytes().as_ptr() as *const libc::c_char;
    let len = name.as_ref().len() as libc::size_t;
    unsafe {
        if libc::sethostname(ptr, len) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn setsid() -> io::Result<u32> {
    unsafe {
        let res = libc::setsid();
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(res as u32)
    }
}

fn mount(source: &str, target: &str, fstype: &str, flags: u64, data: Option<&str>)
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

pub fn pivot_root(new_root: &str, put_old: &str) -> io::Result<()> {
    let new_root = cstr(new_root);
    let put_old = cstr(put_old);
    unsafe {
        if libc::syscall(libc::SYS_pivot_root, new_root.as_ptr(), put_old.as_ptr()) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn umount(path: &str) -> io::Result<()> {
    let path = cstr(path);
    unsafe {
        if libc::umount(path.as_ptr()) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn set_controlling_tty(fd: libc::c_int, force: bool) -> io::Result<()> {
    let flag: libc::c_int = if force { 1 } else { 0 };
    unsafe {
        if libc::ioctl(fd, libc::TIOCSCTTY, flag) == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

pub fn waitpid(pid: libc::pid_t, options: libc::c_int) -> io::Result<i32> {
    let mut status = 0 as libc::c_int;
    unsafe {
        if libc::waitpid(pid, &mut status, options) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(status)
}

pub fn reboot(cmd: libc::c_int) -> io::Result<()> {
    unsafe {
        if libc::reboot(cmd) == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }


}