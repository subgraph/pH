use std::ffi::{CString,OsString};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::mem;
use std::os::unix;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt,OpenOptionsExt,PermissionsExt};
use std::os::linux::fs::MetadataExt;
use std::path::{Path, PathBuf};


use libc;
use crate::devices::virtio_9p::file::{
    P9File, P9_DOTL_RDONLY, P9_DOTL_RDWR, P9_DOTL_WRONLY, translate_p9_flags, Qid
};
use crate::devices::virtio_9p::pdu::PduParser;
use crate::devices::virtio_9p::directory::{Directory, P9DirEntry};


pub enum FsTouch {
    Atime,
    AtimeNow,
    Mtime,
    MtimeNow,
}

pub trait FileSystemOps: Clone+Sync+Send {
    fn read_qid(&self, path: &Path) -> io::Result<Qid>;
    fn write_stat(&self, path: &Path, pp: &mut PduParser) -> io::Result<()>;
    fn open(&self, path: &Path, flags: u32) -> io::Result<P9File>;
    fn create(&self, path: &Path, flags: u32, mode: u32) -> io::Result<P9File>;
    fn write_statfs(&self, path: &Path, pp: &mut PduParser) -> io::Result<()>;
    fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()>;
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()>;
    fn touch(&self, path: &Path, which: FsTouch, tv: (u64, u64)) -> io::Result<()>;
    fn truncate(&self, path: &Path, size: u64) -> io::Result<()>;
    fn readlink(&self, path: &Path) -> io::Result<OsString>;
    fn symlink(&self, target: &Path, linkpath: &Path) -> io::Result<()>;
    fn link(&self, target: &Path, newpath: &Path) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    fn create_dir(&self, path: &Path, mode: u32) -> io::Result<()>;
    fn readdir_populate(&self, path: &Path) -> io::Result<Directory>;
}

#[derive(Clone)]
pub struct FileSystem {
    root: PathBuf,
    readonly: bool,
    euid_root: bool,
}

impl FileSystem {
    pub fn new(root: PathBuf, readonly: bool) -> FileSystem {
        let euid_root = Self::is_euid_root();
        FileSystem { root, readonly, euid_root }
    }

    pub fn is_euid_root() -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    pub fn create_with_flags(path: &Path, flags: u32, mode: u32, is_root: bool) -> io::Result<File> {
        let rdwr = flags & libc::O_ACCMODE as u32;
        let flags = translate_p9_flags(flags, is_root) &!libc::O_TRUNC;
        OpenOptions::new()
            .read(rdwr == P9_DOTL_RDONLY || rdwr == P9_DOTL_RDWR)
            .write(rdwr == P9_DOTL_WRONLY || rdwr == P9_DOTL_RDWR)
            .custom_flags(flags)
            .create_new(true)
            .mode(mode)
            .open(path)
    }

    pub fn open_with_flags(path: &Path, flags: u32, is_root: bool) -> io::Result<File> {
        let rdwr = flags & libc::O_ACCMODE as u32;
        let flags = translate_p9_flags(flags, is_root);

        OpenOptions::new()
            .read(rdwr == P9_DOTL_RDONLY || rdwr == P9_DOTL_RDWR)
            .write(rdwr == P9_DOTL_WRONLY || rdwr == P9_DOTL_RDWR)
            .custom_flags(flags)
            .open(path)
    }

    fn new_file(&self, file: File) -> P9File {
        P9File::from_file(file)
    }

    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        path.symlink_metadata()
    }
}

fn cstr(path: &Path) -> io::Result<CString> {
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

impl FileSystemOps for FileSystem {
    fn read_qid(&self, path: &Path) -> io::Result<Qid> {
        let meta = self.metadata(&path)?;
        let qid = Qid::from_metadata(&meta);
        Ok(qid)
    }

    fn write_stat(&self, path: &Path, pp: &mut PduParser) -> io::Result<()> {
        let meta = self.metadata(path)?;

        const P9_STATS_BASIC: u64 =  0x000007ff;
        pp.w64(P9_STATS_BASIC)?;

        let qid = Qid::from_metadata(&meta);
        qid.write(pp)?;

        pp.w32(meta.st_mode())?;
        pp.w32(meta.st_uid())?;
        pp.w32(meta.st_gid())?;
        pp.w64(meta.st_nlink())?;
        pp.w64(meta.st_rdev())?;
        pp.w64(meta.st_size())?;
        pp.w64(meta.st_blksize())?;
        pp.w64(meta.st_blocks())?;
        pp.w64(meta.st_atime() as u64)?;
        pp.w64(meta.st_atime_nsec() as u64)?;
        pp.w64(meta.st_mtime() as u64)?;
        pp.w64(meta.st_mtime_nsec() as u64)?;
        pp.w64(meta.st_ctime() as u64)?;
        pp.w64(meta.st_ctime_nsec() as u64)?;
        pp.w64(0)?;
        pp.w64(0)?;
        pp.w64(0)?;
        pp.w64(0)?;
        Ok(())
    }

    fn open(&self, path: &Path, flags: u32) -> io::Result<P9File> {
        let file =FileSystem::open_with_flags(&path, flags, self.euid_root)?;
        Ok(self.new_file(file))
    }

    fn create(&self, path: &Path, flags: u32, mode: u32) -> io::Result<P9File> {
        let file = FileSystem::create_with_flags(&path, flags, mode, self.euid_root)?;
        Ok(self.new_file(file))
    }

    fn write_statfs(&self, path: &Path, pp: &mut PduParser) -> io::Result<()> {
        let path_cstr = cstr(&path)?;

        let mut statfs: libc::statfs64 = unsafe { mem::zeroed() };
        unsafe {
            let ret = libc::statfs64(path_cstr.as_ptr(), &mut statfs);
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        pp.w32(statfs.f_type as u32)?;
        pp.w32(statfs.f_bsize as u32)?;
        pp.w64(statfs.f_blocks)?;
        pp.w64(statfs.f_bfree)?;
        pp.w64(statfs.f_bavail)?;
        pp.w64(statfs.f_files)?;
        pp.w64(statfs.f_ffree)?;
        pp.w64(0)?;
        pp.w32(statfs.f_namelen as u32)?;
        Ok(())
    }

    fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()> {
        let path_cstr = cstr(&path)?;
        unsafe {
            if libc::chown(path_cstr.as_ptr(), uid, gid) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        let meta = self.metadata(path)?;
        Ok(meta.permissions().set_mode(mode))
    }

    fn touch(&self, path: &Path, which: FsTouch, tv: (u64, u64)) -> io::Result<()> {
        let path_cstr = cstr(&path)?;

        let tval = libc::timespec {
            tv_sec: tv.0 as i64,
            tv_nsec: tv.1 as i64,
        };
        let omit = libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        };
        let now = libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        };

        let times = match which {
            FsTouch::Atime => [tval, omit],
            FsTouch::AtimeNow => [ now, omit ],
            FsTouch::Mtime => [omit, tval ],
            FsTouch::MtimeNow => [omit, now],
        };
        unsafe {
            if libc::utimensat(-1, path_cstr.as_ptr(), times.as_ptr(), 0) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn truncate(&self, path: &Path, size: u64) -> io::Result<()> {
        let path_cstr = cstr(&path)?;
        unsafe {
            if libc::truncate64(path_cstr.as_ptr(), size as i64) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn readlink(&self, path: &Path) -> io::Result<OsString> {
        fs::read_link(&path).map(|pbuf| pbuf.into_os_string())
    }

    fn symlink(&self, target: &Path, linkpath: &Path) -> io::Result<()> {
        unix::fs::symlink(target, linkpath)
    }

    fn link(&self, target: &Path, newpath: &Path) -> io::Result<()> {
        fs::hard_link(target, newpath)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }

    fn create_dir(&self, path: &Path, mode: u32) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(false)
            .mode(mode & 0o755)
            .create(path)
    }

    fn readdir_populate(&self, path: &Path) -> io::Result<Directory> {
        let mut directory = Directory::new();
        let mut offset = 0;
        for dent in fs::read_dir(path)? {
            let dent = dent?;
            let p9entry = P9DirEntry::from_direntry(dent, offset)?;
            offset = p9entry.offset();
            directory.push_entry(p9entry);
        }
        Ok(directory)
    }
}


