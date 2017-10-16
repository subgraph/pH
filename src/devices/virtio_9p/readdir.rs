use std::path::Path;
use std::mem;
use std::ptr;
use std::io;
use std::ffi::{OsStr,CStr,CString};
use std::os::unix::ffi::OsStrExt;

use libc;

struct Dir(*mut libc::DIR);

pub struct ReadDir {
    dirp: Dir,
    last_pos: i64,
}

pub struct DirEntry {
    entry: libc::dirent64,
}


fn cstr(path: &Path) -> io::Result<CString> {
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

impl ReadDir {
    pub fn open(path: &Path) -> io::Result<ReadDir> {
        let p = cstr(path)?;
        unsafe {
            let ptr = libc::opendir(p.as_ptr());
            if ptr.is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(ReadDir{ dirp: Dir(ptr), last_pos: 0 })
            }
        }
    }

    pub fn tell(&self) -> io::Result<i64> {
        unsafe {
            let loc = libc::telldir(self.dirp.0);
            if loc == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(loc)
        }
    }

    pub fn seek(&self, loc: i64) {
        unsafe { libc::seekdir(self.dirp.0, loc)}
    }

    pub fn fsync(&self) -> io::Result<()> {
        unsafe {
            if libc::fsync(libc::dirfd(self.dirp.0)) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn save_current_pos(&mut self) {
        match self.tell() {
            Ok(loc) => self.last_pos = loc,
            Err(_) => (),
        };
    }

    pub fn restore_last_pos(&mut self) {
        self.seek(self.last_pos)
    }
}


impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        self.save_current_pos();
        unsafe {
            let mut ret = DirEntry {
                entry: mem::zeroed(),
            };
            let mut entry_ptr = ptr::null_mut();
            loop {
                if libc::readdir64_r(self.dirp.0, &mut ret.entry, &mut entry_ptr) != 0 {
                    return Some(Err(io::Error::last_os_error()))
                }
                if entry_ptr.is_null() {
                    return None
                }
                if ret.name_bytes() != b"." && ret.name_bytes() != b".." {
                    return Some(Ok(ret))
                }
            }
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = unsafe { libc::closedir(self.0) };
    }
}


impl DirEntry {
    #[allow(dead_code)]
    pub fn file_name(&self) -> &OsStr {
        OsStr::from_bytes(self.name_bytes())
    }

    pub fn offset(&self) -> u64 {
        self.entry.d_off as u64
    }

    pub fn file_type(&self) -> u8 {
        self.entry.d_type
    }

    pub fn ino(&self) -> u64 {
        self.entry.d_ino as u64
    }

    pub fn name_bytes(&self) -> &[u8] {
        unsafe {
            CStr::from_ptr(self.entry.d_name.as_ptr()).to_bytes()
        }
    }
}

