use std::mem;
use std::ffi::CString;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::fs::{self,File,Metadata,OpenOptions};

use std::io;
use std::path::{PathBuf,Path,Component};

use std::os::unix::fs::OpenOptionsExt;

use libc;

use super::readdir::ReadDir;

const MAX_SYMLINKS: usize = 16;
const PATH_MAX: usize = 1024; // it's actually 4096 on linux

const O_RDONLY: u32 = 0;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_ACCMODE: u32 = 0x3;
const ALLOWED_FLAGS: u32 = (libc::O_APPEND | libc::O_TRUNC | libc::O_LARGEFILE
                            | libc::O_DIRECTORY | libc::O_DSYNC | libc::O_NOFOLLOW
                            | libc::O_SYNC) as u32;

#[derive(Default)]
pub struct StatFs {
    pub f_type: u32,
    pub f_bsize: u32,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub fsid: u64,
    pub f_namelen: u32,
}
impl StatFs {
    fn new() -> StatFs {
        StatFs { ..Default::default() }
    }
}

pub enum FsTouch {
    Atime,
    AtimeNow,
    Mtime,
    MtimeNow,
}

pub trait FileSystemOps {
    fn open(&self, path: &Path, flags: u32) -> io::Result<FileDescriptor>;
    fn open_dir(&self, path: &Path) -> io::Result<FileDescriptor>;
    fn create(&self, path: &Path, flags: u32, mode: u32) -> io::Result<FileDescriptor>;
    fn stat(&self, path: &Path) -> io::Result<Metadata>;
    fn statfs(&self, path: &Path) -> io::Result<StatFs>;
    fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()>;
    fn chmod(&self, path: &Path, mode: u32) -> io::Result<()>;
    fn touch(&self, path: &Path, which: FsTouch, tv: (u64, u64)) -> io::Result<()>;
    fn truncate(&self, path: &Path, size: u64) -> io::Result<()>;
    fn readlink(&self, path: &Path) -> io::Result<OsString>;
   // fn symlink(&self, target: &Path, linkpath: &Path) -> io::Result<()>;
}

#[derive(Clone)]
pub struct FileSystem {
    init_path: PathBuf,
    resolver: PathResolver,
    readonly: bool,
}

pub enum FileDescriptor {
    None,
    Dir(ReadDir),
    File(File),
}

impl FileDescriptor {
    #[allow(dead_code)]
    pub fn is_file(&self) -> bool {
        match *self {
            FileDescriptor::File(..) => true,
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn is_dir(&self) -> bool {
        match *self {
            FileDescriptor::Dir(..) => true,
            _ => false,
        }
    }

    pub fn borrow_file(&mut self) -> io::Result<&mut File> {
        match *self {
            FileDescriptor::File(ref mut file_ref) => Ok(file_ref),
            _ => Err(os_err(libc::EBADF)),
        }
    }

    pub fn borrow_dir(&mut self) -> io::Result<&mut ReadDir> {
        match *self {
            FileDescriptor::Dir(ref mut dir_ref) => Ok(dir_ref),
            _ => Err(os_err(libc::EBADF)),
        }
    }
}

impl FileSystem {
    pub fn new(root: PathBuf, init_path: PathBuf, readonly: bool) -> FileSystem {
        FileSystem { resolver: PathResolver::new(root), init_path, readonly }
    }

    fn fullpath(&self, path: &Path) -> io::Result<PathBuf> {
        if path.to_str().unwrap() == "/phinit" {
            return Ok(self.init_path.clone())
        }
        self.resolver.fullpath(path)
    }


    fn flags_to_open_options(&self, flags: u32) -> io::Result<OpenOptions> {
        let acc = flags & O_ACCMODE;
        let mut oo = OpenOptions::new();

        if self.readonly && acc != O_RDONLY {
            return Err(io::Error::from_raw_os_error(libc::EACCES));
        }

        match acc {
            O_RDONLY => { oo.read(true).write(false); }
            O_WRONLY => { oo.read(false).write(true); }
            O_RDWR   => { oo.read(true).write(true); }
            _ => return Err(os_err(libc::EINVAL))
        }


        // There should never be a symlink in path but add O_NOFOLLOW anyways
        let custom = libc::O_NOFOLLOW  | (flags & ALLOWED_FLAGS) as i32;
        oo.custom_flags(custom);
        Ok(oo)
    }
}

///
/// Resolves paths into a canonical path which is always no higher
/// than the `root` path.
#[derive(Clone)]
struct PathResolver {
    root: PathBuf,
}

impl PathResolver {
    fn new(root: PathBuf) -> PathResolver {
        // root must be absolute path
        PathResolver{ root }
    }


    ///
    /// Canonicalize `path` so that .. segments in both in
    /// the path itself and any symlinks in the path do
    /// not escape.  The returned path will not contain any
    /// symlinks and refers to a path which is a subdirectory
    /// of `self.root`
    fn resolve_path(&self, path: &Path) -> io::Result<PathBuf> {
        let mut buf = PathBuf::from(path);
        let mut nlinks = 0_usize;
        while self._resolve(&mut buf)? {
            nlinks += 1;
            if nlinks > MAX_SYMLINKS {
                return Err(io::Error::from_raw_os_error(libc::ELOOP))
            }
            if buf.as_os_str().len() > PATH_MAX {
                return Err(io::Error::from_raw_os_error(libc::ENAMETOOLONG))
            }
        }
        Ok(buf)
    }

    fn is_path_symlink(path: &Path) -> bool {
        match path.symlink_metadata() {
            Ok(meta) => meta.file_type().is_symlink(),
            Err(..) => false
        }
    }

    fn fullpath(&self, path: &Path) -> io::Result<PathBuf> {
        let resolved = self.resolve_path(path)?;
        Ok(self.realpath(&resolved))
    }

    fn realpath(&self, path: &Path) -> PathBuf {
        let mut cs = path.components();
        if path.is_absolute() {
            cs.next();
        }
        self.root.join(cs.as_path())
    }

    fn resolve_symlink(&self, path: &mut PathBuf) -> io::Result<bool> {
        let realpath = self.realpath(path);
        if PathResolver::is_path_symlink(&realpath) {
            path.pop();
            path.push(realpath.read_link()?);
            return Ok(true)
        }
        Ok(false)
    }

    fn resolve_component(&self, c: Component, pathbuf: &mut PathBuf) -> io::Result<bool> {
        match c {
            Component::RootDir => pathbuf.push("/"),
            Component::CurDir | Component::Prefix(..) => (),
            Component::ParentDir => { pathbuf.pop(); },
            Component::Normal(name) => {
                pathbuf.push(name);
                let link = self.resolve_symlink(pathbuf)?;
                return Ok(link)
            }
        };
        Ok(false)
    }

    fn _resolve(&self, path: &mut PathBuf) -> io::Result<bool> {
        let copy = (*path).clone();
        let mut components = copy.components();

        path.push("/");

        while let Some(c) = components.next() {
            if self.resolve_component(c, path)? {
                let tmp = path.join(components.as_path());
                path.push(tmp);
                return Ok(true)
            }
        }
        Ok(false)
    }
}

fn cstr(path: &Path) -> io::Result<CString> {
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

impl FileSystemOps for FileSystem {
    fn open(&self, path: &Path, flags: u32) -> io::Result<FileDescriptor> {
        let fullpath = self.fullpath(path)?;
        let meta = fullpath.metadata()?;
        if meta.is_dir() {
            let read_dir = ReadDir::open(&fullpath)?;
            return Ok(FileDescriptor::Dir(read_dir))
        }

        let options = self.flags_to_open_options(flags)?;
        let file = options.open(&fullpath)?;
        return Ok(FileDescriptor::File(file))
    }

    fn create(&self, path: &Path, flags: u32, mode: u32) -> io::Result<FileDescriptor> {
        let fullpath = self.fullpath(path)?;
        let mut options = self.flags_to_open_options(flags)?;
        options.create(true);
        options.mode(mode & 0o777);
        let file = options.open(&fullpath)?;
        return Ok(FileDescriptor::File(file))
    }

    fn open_dir(&self, path: &Path) -> io::Result<FileDescriptor> {
        let fullpath = self.fullpath(path)?;
        let read_dir = ReadDir::open(&fullpath)?;
        return Ok(FileDescriptor::Dir(read_dir))
    }

    fn stat(&self, path: &Path) -> io::Result<Metadata> {
        let fullpath = self.fullpath(path)?;
        let meta = fullpath.metadata()?;
        Ok(meta)
    }

    fn statfs(&self, path: &Path) -> io::Result<StatFs> {
        let fullpath = self.fullpath(path)?;
        let path_cstr = cstr(&fullpath)?;
        let mut stat: LibcStatFs;
        unsafe {
            stat = mem::zeroed();
            let ret = statfs(path_cstr.as_ptr(), &mut stat);
            if ret < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut statfs = StatFs::new();
        statfs.f_type = stat.f_type as u32;
        statfs.f_bsize = stat.f_bsize as u32;
        statfs.f_blocks = stat.f_blocks;
        statfs.f_bfree = stat.f_bfree;
        statfs.f_bavail = stat.f_bavail;
        statfs.f_files = stat.f_files;
        statfs.f_ffree = stat.f_ffree;
        statfs.f_namelen = stat.f_namelen as u32;
        statfs.fsid = stat.f_fsid.val[0] as u64 | ((stat.f_fsid.val[1] as u64) << 32);

        Ok(statfs)

    }
    fn chown(&self, path: &Path, uid: u32, gid: u32) -> io::Result<()> {
        let fullpath = self.fullpath(path)?;
        let path_cstr = cstr(&fullpath)?;
        unsafe {
            if libc::chown(path_cstr.as_ptr(), uid, gid) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn chmod(&self, path: &Path, mode: u32) -> io::Result<()> {
        // XXX see std::os::unix::fs::PermissionsExt for a better way
        let fullpath = self.fullpath(path)?;
        let path_cstr = cstr(&fullpath)?;
        unsafe {
            if libc::chmod(path_cstr.as_ptr(), mode) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn touch(&self, path: &Path, which: FsTouch, tv: (u64, u64)) -> io::Result<()> {
        let fullpath = self.fullpath(path)?;
        let path_cstr = cstr(&fullpath)?;

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
            // XXX this could be wildly wrong but libc has wrong type
            if libc::utimensat(-1, path_cstr.as_ptr(), &times.as_ptr() as *const _ as *const libc::timespec, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    fn truncate(&self, path: &Path, size: u64) -> io::Result<()> {
        let fullpath = self.fullpath(path)?;
        let path_cstr = cstr(&fullpath)?;
        unsafe {
            if libc::truncate64(path_cstr.as_ptr(), size as i64) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    // XXX
    fn readlink(&self, path: &Path) -> io::Result<OsString> {
        let fullpath = self.fullpath(path)?;
        fs::read_link(&fullpath).map(|pbuf| pbuf.into_os_string())
    }
}



#[repr(C)]
pub struct LibcStatFs {
    f_type: u64,
    f_bsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,

    f_files: u64,
    f_ffree: u64,
    f_fsid: FsidT,

    f_namelen: u64,
    f_frsize: u64,
    f_spare: [u64; 5],
}

#[repr(C)]
struct FsidT{
    val: [libc::c_int; 2],
}
extern {
    pub fn statfs(path: *const libc::c_char, buf: *mut LibcStatFs) -> libc::c_int;
}

fn os_err(errno: i32) -> io::Error {
   io::Error::from_raw_os_error(errno)
}


