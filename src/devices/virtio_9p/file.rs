use std::cell::{RefCell, RefMut, Cell};
use std::collections::BTreeMap;
use std::{io, fmt};
use std::path::{Path, PathBuf, Component};
use std::fs::{Metadata, File};
use std::os::unix::io::{RawFd,AsRawFd};
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs::FileExt;

use crate::devices::virtio_9p::{
    pdu::PduParser, directory::Directory, filesystem::FileSystemOps,
};
use std::io::{Cursor, SeekFrom, Seek, Read};
use std::sync::{RwLock, Arc};

pub const P9_DOTL_RDONLY: u32        = 0o00000000;
pub const P9_DOTL_WRONLY: u32        = 0o00000001;
pub const P9_DOTL_RDWR: u32          = 0o00000002;
const _P9_DOTL_NOACCESS: u32      = 0o00000003;
const P9_DOTL_CREATE: u32        = 0o00000100;
const P9_DOTL_EXCL: u32          = 0o00000200;
const P9_DOTL_NOCTTY: u32        = 0o00000400;
const P9_DOTL_TRUNC: u32         = 0o00001000;
const P9_DOTL_APPEND: u32        = 0o00002000;
const P9_DOTL_NONBLOCK: u32      = 0o00004000;
const P9_DOTL_DSYNC: u32         = 0o00010000;
const P9_DOTL_FASYNC: u32        = 0o00020000;
const P9_DOTL_DIRECT: u32        = 0o00040000;
const P9_DOTL_LARGEFILE: u32     = 0o00100000;
const P9_DOTL_DIRECTORY: u32     = 0o00200000;
const P9_DOTL_NOFOLLOW: u32      = 0o00400000;
const P9_DOTL_NOATIME: u32       = 0o01000000;
const _P9_DOTL_CLOEXEC: u32       = 0o02000000;
const P9_DOTL_SYNC: u32          = 0o04000000;

pub const P9_QTFILE: u8 = 0x00;
pub const P9_QTSYMLINK: u8 = 0x02;
pub const P9_QTDIR: u8 = 0x80;

const P9_LOCK_SUCCESS: u8 = 0;
const P9_LOCK_BLOCKED: u8 =1;
const P9_LOCK_ERROR: u8 = 2;

const P9_LOCK_TYPE_RDLCK: u8 = 0;
const P9_LOCK_TYPE_WRLCK: u8 = 1;
const P9_LOCK_TYPE_UNLCK: u8 = 2;

#[derive(PartialEq,Copy,Clone)]
enum LockType {
    LockUn,
    LockSh,
    LockEx,
}

#[derive(Clone)]
pub struct Buffer<T: AsRef<[u8]>>(Arc<RwLock<Cursor<T>>>);
impl <T: AsRef<[u8]>> Buffer <T> {
    pub fn new(bytes: T) -> Self {
        Buffer(Arc::new(RwLock::new(Cursor::new(bytes))))
    }

    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        let mut lock = self.0.write().unwrap();
        lock.seek(SeekFrom::Start(offset))?;
        lock.read(buffer)
    }
    pub fn write_at(&self, _buffer: &[u8], _offset: u64) -> io::Result<usize> {
        return Err(io::Error::from_raw_os_error(libc::EPERM))
    }

}

enum FileObject {
    File(File),
    BufferFile(Buffer<&'static [u8]>),
    NotAFile,
}

impl FileObject {
    fn fd(&self) -> Option<RawFd> {
        match self {
            FileObject::File(file) => Some(file.as_raw_fd()),
            _ => None,
        }
    }
}

pub struct P9File {
    file: FileObject,
    lock: Cell<LockType>,
}

impl P9File {

    fn new(file: FileObject) -> Self {
        P9File { file, lock: Cell::new(LockType::LockUn) }
    }
    pub fn new_not_a_file() -> Self {
        Self::new(FileObject::NotAFile)
    }

    pub fn from_file(file: File) -> Self {
        Self::new(FileObject::File(file))
    }

    pub fn from_buffer(buffer: Buffer<&'static [u8]>) -> Self {
        Self::new(FileObject::BufferFile(buffer))
    }

    pub fn sync_all(&self) -> io::Result<()> {
        match self.file {
            FileObject::File(ref f) => f.sync_all(),
            _ => Ok(()),
        }
    }

    pub fn sync_data(&self) -> io::Result<()> {
        match self.file {
            FileObject::File(ref f) => f.sync_data(),
            _ => Ok(()),
        }
    }

    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        match self.file {
            FileObject::File(ref f) => f.read_at(buffer,offset),
            FileObject::BufferFile(ref f) => f.read_at(buffer, offset),
            FileObject::NotAFile =>  Ok(0),
        }
    }

    pub fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<usize> {
        match self.file {
            FileObject::File(ref f) => f.write_at(buffer,offset),
            FileObject::BufferFile(ref f) => f.write_at(buffer, offset),
            FileObject::NotAFile =>  Ok(0),
        }
    }

    fn map_locktype(ltype: u8) -> LockType {
        match ltype {
            P9_LOCK_TYPE_UNLCK => LockType::LockUn,
            P9_LOCK_TYPE_RDLCK => LockType::LockSh,
            P9_LOCK_TYPE_WRLCK => LockType::LockEx,
            _ => LockType::LockUn,
        }
    }

    fn errno() -> i32 {
        unsafe { *libc::__errno_location() }
    }

    fn raw_flock(fd: RawFd, op: i32) -> u8 {
        unsafe {
            if libc::flock(fd, op) == -1 {
                if Self::errno() == libc::EWOULDBLOCK {
                    P9_LOCK_BLOCKED
                } else {
                    P9_LOCK_ERROR
                }
            } else {
                P9_LOCK_SUCCESS
            }
        }
    }

    pub fn flock(&self, ltype: u8) -> io::Result<u8> {

        let fd = match self.file.fd() {
            Some(fd) => fd,
            None => {
                self.lock.set(Self::map_locktype(ltype));
                return Ok(P9_LOCK_SUCCESS);
            }
        };

        match ltype {
            P9_LOCK_TYPE_UNLCK => {
                let status = Self::raw_flock(fd, libc::LOCK_UN);
                if status == P9_LOCK_SUCCESS {
                    self.lock.set(LockType::LockUn);
                }
                Ok(status)
            }
            P9_LOCK_TYPE_WRLCK => {
                let status = Self::raw_flock(fd, libc::LOCK_EX|libc::LOCK_NB);
                if status == P9_LOCK_SUCCESS {
                    self.lock.set(LockType::LockEx);
                }
                Ok(status)
            }
            P9_LOCK_TYPE_RDLCK => {
                let status = Self::raw_flock(fd, libc::LOCK_SH|libc::LOCK_NB);
                if status == P9_LOCK_SUCCESS {
                    self.lock.set(LockType::LockSh);
                }
                Ok(status)
            }
            _ => system_error(libc::EINVAL),
        }
    }

    pub fn getlock(&self, ltype: u8) -> io::Result<u8> {
        let fd = match self.file.fd() {
            Some(fd) => fd,
            None => {
                return Ok(P9_LOCK_TYPE_UNLCK);
            }
        };

        match ltype {
            P9_LOCK_TYPE_RDLCK => {
                if self.lock.get() == LockType::LockUn {
                    match Self::raw_flock(fd, libc::LOCK_NB|libc::LOCK_SH) {
                        P9_LOCK_SUCCESS => { Self::raw_flock(fd, libc::LOCK_UN); }
                        _ => return Ok(P9_LOCK_TYPE_WRLCK),
                    }
                }
            }
            P9_LOCK_TYPE_WRLCK => {
                if self.lock.get() == LockType::LockUn {
                    match Self::raw_flock(fd, libc::LOCK_NB|libc::LOCK_EX) {
                        P9_LOCK_SUCCESS => { Self::raw_flock(fd, libc::LOCK_UN); },
                        _ => return Ok(P9_LOCK_TYPE_WRLCK),
                    }
                }
            }
            _ => {}
        }
        Ok(P9_LOCK_TYPE_UNLCK)
    }
}

#[derive(Copy,Clone)]
pub struct Qid {
    qtype: u8,
    version: u32,
    path: u64,
}

impl Qid {

    pub fn new(qtype: u8, version: u32, path: u64) -> Qid {
        Qid { qtype, version, path }
    }

    pub fn from_metadata(meta: &Metadata) -> Qid {
        let qtype = if meta.is_dir() {
            P9_QTDIR
        } else if meta.is_file() {
            P9_QTFILE
        } else if meta.file_type().is_symlink() {
            P9_QTSYMLINK
        } else {
            0
        };
        let version = meta.st_mtime() as u32 ^ (meta.st_size() << 8) as u32;
        let path = meta.st_ino();
        Qid::new(qtype, version, path)
    }

    pub fn is_dir(&self) -> bool {
        self.qtype == P9_QTDIR
    }

    pub fn write(&self, pp: &mut PduParser) -> io::Result<()> {
        pp.w8(self.qtype)?;
        pp.w32(self.version)?;
        pp.w64(self.path)?;
        Ok(())
    }
}

pub fn translate_p9_flags(flags: u32, is_root: bool) -> libc::c_int {
    let flagmap = &[
        (P9_DOTL_CREATE, libc::O_CREAT),
        (P9_DOTL_EXCL, libc::O_EXCL),
        (P9_DOTL_NOCTTY, libc::O_NOCTTY),
        (P9_DOTL_TRUNC, libc::O_TRUNC),
        (P9_DOTL_APPEND, libc::O_APPEND),
        (P9_DOTL_NONBLOCK, libc::O_NONBLOCK),
        (P9_DOTL_DSYNC, libc::O_DSYNC),
        (P9_DOTL_FASYNC, libc::O_ASYNC),
        (P9_DOTL_DIRECT, libc::O_DIRECT),
        (P9_DOTL_LARGEFILE, libc::O_LARGEFILE),
        (P9_DOTL_DIRECTORY, libc::O_DIRECTORY),
        (P9_DOTL_NOFOLLOW, libc::O_NOFOLLOW),
        (P9_DOTL_SYNC, libc::O_SYNC),
    ];
    let mut custom = flagmap.iter()
        .fold(0, |acc, (a,b)|
            if flags & *a != 0 { acc | *b } else { acc });

    if is_root && flags & P9_DOTL_NOATIME != 0 {
        custom |= libc::O_NOATIME;
    }
    /* copied from qemu */
    custom &= !(libc::O_NOCTTY|libc::O_ASYNC|libc::O_CREAT);
    custom &= !libc::O_DIRECT;
    custom
}

pub struct Fids<T: FileSystemOps> {
    ops: T,
    root: PathBuf,
    fidmap: BTreeMap<u32, Fid<T>>,
}

impl <T: FileSystemOps> Fids<T> {
    pub fn new(root: PathBuf, ops: T) -> Self {
        Fids {
            ops,
            root,
            fidmap: BTreeMap::new(),
        }
    }

    pub fn fid(&self, id: u32) -> io::Result<&Fid<T>> {
        self.fidmap.get(&id).ok_or(Self::bad_fd_error())
    }

    pub fn fid_mut(&mut self, id: u32) -> io::Result<&mut Fid<T>> {
        self.fidmap.get_mut(&id).ok_or(Self::bad_fd_error())
    }

    pub fn read_fid(&self, pp: &mut PduParser) -> io::Result<&Fid<T>> {
        let id = pp.r32()?;
        self.fid(id)
    }

    pub fn read_new_path(&self, pp: &mut PduParser) -> io::Result<PathBuf> {
        let fid = self.read_fid(pp)?;
        let name = pp.read_string()?;
        fid.join_name(&self.root, &name)
    }

    pub fn path_join_name(&self, qid: Qid, path: &Path, name: &str) -> io::Result<PathBuf> {
        Fid::<T>::path_join_name(qid, path, &self.root, name)
    }

    pub fn clear(&mut self) {
        self.fidmap.clear()
    }

    pub fn add(&mut self, fid: Fid<T>) {
        self.fidmap.insert(fid.id, fid);
    }

    pub fn exists(&self, id: u32) -> bool {
        self.fidmap.contains_key(&id)
    }

    pub fn remove(&mut self, id: u32) -> io::Result<Fid<T>> {
        match self.fidmap.remove(&id) {
            Some(fid) => Ok(fid),
            None => Err(Self::bad_fd_error())
        }
    }

    pub fn create<P: Into<PathBuf>>(&self, id: u32, path: P) -> io::Result<Fid<T>> {
        Fid::create(self.ops.clone(), id, path)
    }

    pub fn read_qid(&self, path: &Path) -> io::Result<Qid> {
        self.ops.read_qid(path)
    }

    fn bad_fd_error() -> io::Error {
        io::Error::from_raw_os_error(libc::EBADF)
    }
}

pub struct Fid<T: FileSystemOps> {
    ops: T,
    id: u32,
    path: PathBuf,
    qid: Qid,
    file: Option<P9File>,
    directory: RefCell<Option<Directory>>,
}

impl <T: FileSystemOps> Fid<T> {
    fn create<P: Into<PathBuf>>(ops: T, id: u32, path: P) -> io::Result<Self> {
        let path = path.into();
        let qid = ops.read_qid(&path)?;
        Ok(Fid {
            ops, id, path, qid,
            file: None,
            directory: RefCell::new(None),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn qid(&self) -> Qid {
        self.qid
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn write_stat(&self, pp: &mut PduParser) -> io::Result<()> {
        self.ops.write_stat(self.path(), pp)
    }

    pub fn reload_qid(&mut self) -> io::Result<()> {
        self.qid = self.ops.read_qid(self.path())?;
        Ok(())
    }

    pub fn set_file(&mut self, file: P9File) {
        self.file = Some(file)
    }

    pub fn set_path<P: Into<PathBuf>>(&mut self, path: P) -> io::Result<()> {
        self.path = path.into();
        self.reload_qid()
    }

    pub fn write_qid(&self, pp: &mut PduParser) -> io::Result<()> {
        self.qid.write(pp)
    }

    pub fn is_dir(&self) -> bool {
        self.qid.is_dir()
    }

    pub fn file(&self) -> io::Result<&P9File> {
        match self.file.as_ref() {
            Some(file) => Ok(file),
            None => system_error(libc::EBADF),
        }
    }

    pub fn join_name(&self, root: &Path, name: &str) -> io::Result<PathBuf> {
        Self::path_join_name(self.qid, self.path(), root, name)
    }

    fn path_join_name(qid: Qid, path: &Path, root: &Path, name: &str) -> io::Result<PathBuf> {
        if !qid.is_dir() {
            return system_error(libc::ENOTDIR);
        }
        let p= Path::new(name);

        if p.components().count() > 1 {
            return system_error(libc::EINVAL);
        }

        let mut path = path.to_path_buf();
        match p.components().next() {
            Some(Component::ParentDir) => {
                path.pop();
                if !path.starts_with(root) {
                    return system_error(libc::EINVAL);
                }
            }
            Some(Component::Normal(name)) => path.push(name),
            None => {},
            _ => return system_error(libc::EINVAL),
        };
        Ok(path)
    }

    pub fn load_directory(&self) -> io::Result<()> {
        if !self.is_dir() {
            return system_error(libc::ENOTDIR);
        }
        let dir = self.ops.readdir_populate(self.path())?;
        self.directory.replace(Some(dir));
        Ok(())
    }

    pub fn directory(&self) -> RefMut<Option<Directory>>{
        self.directory.borrow_mut()
    }
}

impl <T: FileSystemOps> fmt::Display for Fid<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fid:({}, id={})", self.path().display(), self.id)
    }
}

fn system_error<T>(errno: libc::c_int) -> io::Result<T> {
    Err(io::Error::from_raw_os_error(errno))
}
