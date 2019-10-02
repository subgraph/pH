use std::path::{PathBuf, Path};
use std::{io, cmp};

use crate::devices::virtio_9p::{
    pdu::{PduParser, P9Attr},
    filesystem::{FileSystemOps, FsTouch},
    file::{Fids, Fid, Qid},
};

const P9_TSTATFS: u8      = 8;
const P9_TLOPEN: u8       = 12;
const P9_TLCREATE: u8     = 14;
const P9_TSYMLINK: u8     = 16;
const P9_TMKNOD: u8       = 18;
const P9_TRENAME: u8      = 20;
const P9_TREADLINK: u8    = 22;
const P9_TGETATTR: u8     = 24;
const P9_TSETATTR: u8     = 26;
const P9_TXATTRWALK: u8   = 30;
const P9_TXATTRCREATE: u8 = 32;
const P9_TREADDIR: u8     = 40;
const P9_TFSYNC: u8       = 50;
const P9_TLOCK: u8        = 52;
const P9_TGETLOCK: u8     = 54;
const P9_TLINK: u8        = 70;
const P9_TMKDIR: u8       = 72;
const P9_TRENAMEAT: u8    = 74;
const P9_TUNLINKAT: u8    = 76;
const P9_TVERSION:u8      = 100;
const P9_TATTACH :u8      = 104;
const P9_TFLUSH: u8       = 108;
const P9_TWALK :u8        = 110;
const P9_TREAD: u8        = 116;
const P9_TWRITE: u8       = 118;
const P9_TCLUNK: u8       = 120;
const P9_REMOVE: u8       = 122;


const P9_LOCK_FLAGS_BLOCK: u32 = 1;

pub struct Server<T: FileSystemOps> {
    root: PathBuf,
    debug: bool,
    msize: u32,
    fids: Fids<T>,
    filesystem: T,
}

fn system_error<T>(errno: libc::c_int) -> io::Result<T> {
    Err(io::Error::from_raw_os_error(errno))
}

impl <T: FileSystemOps> Server<T> {

    pub fn new(root: &Path, filesystem: T) -> Self {
        let root = root.to_owned();
        let fids = Fids::new(root.clone(), filesystem.clone());
        Server {
            root,
            debug: false,
            msize: 0,
            fids,
            filesystem
        }
    }

    pub fn enable_debug(&mut self) {
        self.debug = true;
    }

    fn fid_mut(&mut self, id: u32) -> io::Result<&mut Fid<T>> {
        self.fids.fid_mut(id)
    }

    fn read_fid(&self, pp: &mut PduParser) -> io::Result<&Fid<T>> {
        self.fids.read_fid(pp)
    }

    /// Reads a directory fid and a string together and constructs a new path by
    /// joining fid with name
    fn read_new_path(&self, pp: &mut PduParser) -> io::Result<PathBuf> {
        self.fids.read_new_path(pp)
    }

    pub fn handle(&mut self, pp: &mut PduParser) {
        match pp.command() {
            Ok(cmd) => {
                if let Err(err) = self.dispatch(cmd, pp) {
                    if self.debug {
                        notify!("error handling command: {}", err);
                    }
                    let _ = pp.bail_err(err);
                }
            }
            Err(e) => {
                warn!("Error reading p9 command: {}", e);
            }
        }
    }

    fn dispatch(&mut self, cmd: u8, pp: &mut PduParser) -> io::Result<()> {
        match cmd {
            P9_TSTATFS => self.p9_statfs(pp)?,
            P9_TLOPEN => self.p9_open(pp)?,
            P9_TLCREATE => self.p9_create(pp)?,
            P9_TSYMLINK => self.p9_symlink(pp)?,
            P9_TMKNOD => self.p9_mknod(pp)?,
            P9_TRENAME => self.p9_rename(pp)?,
            P9_TREADLINK => self.p9_readlink(pp)?,
            P9_TGETATTR => self.p9_getattr(pp)?,
            P9_TSETATTR => self.p9_setattr(pp)?,
            P9_TXATTRWALK => self.p9_unsupported(pp)?,
            P9_TXATTRCREATE =>  self.p9_unsupported(pp)?,
            P9_TREADDIR => self.p9_readdir(pp)?,
            P9_TFSYNC => self.p9_fsync(pp)?,
            P9_TLOCK => self.p9_lock(pp)?,
            P9_TGETLOCK => self.p9_getlock(pp)?,
            P9_TUNLINKAT => self.p9_unlinkat(pp)?,
            P9_TLINK => self.p9_link(pp)?,
            P9_TMKDIR=> self.p9_mkdir(pp)?,
            P9_TRENAMEAT => self.p9_renameat(pp)?,
            P9_TVERSION => self.p9_version(pp)?,
            P9_TATTACH => self.p9_attach(pp)?,
            P9_TFLUSH => self.p9_flush(pp)?,
            P9_TWALK => self.p9_walk(pp)?,
            P9_TREAD => self.p9_read(pp)?,
            P9_TWRITE => self.p9_write(pp)?,
            P9_TCLUNK => self.p9_clunk(pp)?,
            P9_REMOVE => self.p9_remove(pp)?,
            n => warn!("unhandled 9p command: {}", n),
        }
        Ok(())
    }

    fn p9_statfs_args(&self, pp: &mut PduParser) -> io::Result<&Fid<T>> {
        let fid = self.read_fid(pp)?;
        pp.read_done()?;
        Ok(fid)
    }

    fn p9_statfs(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = self.p9_statfs_args(pp)?;

        if self.debug {
            notify!("p9_statfs({})", fid)
        }
        self.filesystem.write_statfs(fid.path(), pp)?;
        pp.write_done()
    }

    fn p9_open_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u32)> {
        let fid = self.read_fid(pp)?;
        let flags = pp.r32()?;
        pp.read_done()?;
        Ok((fid, flags))
    }

    fn p9_open(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, flags) = self.p9_open_args(pp)?;

        if self.debug {
            notify!("p9_open({}, {:08x})", fid, flags)
        }

        let file = self.filesystem.open(fid.path(), flags)?;

        let id = fid.id();
        let fid = self.fid_mut(id)?;

        fid.set_file(file);
        fid.write_qid(pp)?;
        // iounit
        pp.w32(0)?;
        pp.write_done()
    }

    fn p9_create_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, PathBuf, u32, u32)> {
        let dfid = self.read_fid(pp)?;
        let name = pp.read_string()?;
        let path = dfid.join_name(&self.root, &name)?;
        let flags = pp.r32()?;
        let mode = pp.r32()?;
        let _gid = pp.r32()?;
        pp.read_done()?;
        Ok((dfid, path, flags, mode))
    }

    fn p9_create(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (dfid, path, flags, mode) = self.p9_create_args(pp)?;

        if self.debug {
            notify!("p9_create({:?}, flags={:08x}, mode={:04o})",
                    path, flags, mode)
        }

        let file = self.filesystem.create(&path, flags, mode)?;

        let id = dfid.id();
        let dfid = self.fid_mut(id)?;

        dfid.set_path(path)?;
        dfid.set_file(file);

        dfid.write_qid(pp)?;
        // iounit
        pp.w32(0)?;
        pp.write_done()
    }

    fn p9_symlink_args(&self, pp: &mut PduParser) -> io::Result<(PathBuf, String)> {
        let newpath = self.read_new_path(pp)?;
        let target = pp.read_string()?;
        let _gid = pp.r32()?;
        pp.read_done()?;
        Ok((newpath, target))
    }

    fn p9_symlink(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (newpath, target) = self.p9_symlink_args(pp)?;

        if self.debug {
            notify!("p9_symlink({:?}, {})", newpath, target)
        }

        self.filesystem.symlink(&Path::new(&target), &newpath)?;

        self.filesystem.write_stat(&newpath, pp)?;
        pp.write_done()
    }

    fn p9_mknod_args(&self, pp: &mut PduParser) -> io::Result<(PathBuf, u32, u32, u32)> {
        let path = self.read_new_path(pp)?;
        let mode = pp.r32()?;
        let major = pp.r32()?;
        let minor = pp.r32()?;
        let _gid = pp.r32()?;
        pp.read_done()?;
        Ok((path, mode, major, minor))
    }

    fn p9_mknod(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (path, mode, major, minor) = self.p9_mknod_args(pp)?;
        if self.debug {
            notify!("p9_mknod({:?}, {:04o}, {}:{})", path, mode, major, minor)
        }
        system_error(libc::EACCES)
    }

    fn p9_rename_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, PathBuf)> {
        let oldfid = self.read_fid(pp)?;
        let newpath = self.read_new_path(pp)?;
        pp.read_done()?;
        Ok((oldfid, newpath))
    }

    fn p9_rename(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (oldfid, newpath) = self.p9_rename_args(pp)?;
        if self.debug {
            format!("p9_rename({}, {:?})", oldfid, newpath);
        }
        self.filesystem.rename(oldfid.path(), &newpath)?;
        let id = oldfid.id();
        let oldfid = self.fid_mut(id)?;
        oldfid.set_path(newpath)?;
        pp.write_done()
    }

    fn p9_readlink_args(&self, pp: &mut PduParser) -> io::Result<&Fid<T>> {
        let fid = self.read_fid(pp)?;
        pp.read_done()?;
        Ok(fid)
    }

    fn p9_readlink(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = self.p9_readlink_args(pp)?;

        if self.debug {
            notify!("p9_readlink({})", fid);
        }

        let s = self.filesystem.readlink(fid.path())?;
        pp.write_os_string(&s)?;
        pp.write_done()
    }

    fn p9_getattr_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u64)> {
        let fid = self.read_fid(pp)?;
        let mask = pp.r64()?;
        pp.read_done()?;
        Ok((fid, mask))
    }

    fn p9_getattr(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid,mask) = self.p9_getattr_args(pp)?;

        if self.debug {
            notify!("p9_getattr({}, {})", fid, mask);
        }

        // XXX mask?
        fid.write_stat(pp)?;
        if let Err(err) = fid.write_stat(pp) {
            notify!("error from write_stat: {}", err);
            return Err(err);
        }
        pp.write_done()
    }

    fn p9_setattr_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, P9Attr)> {
        let fid = self.read_fid(pp)?;
        let attr = pp.read_attr()?;
        pp.read_done()?;
        Ok((fid, attr))
    }

    fn p9_setattr(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, attr) = self.p9_setattr_args(pp)?;

        if self.debug {
            notify!("p9_setattr({}, {:?})", fid, attr);
        }

        if attr.has_mode() {
            self.filesystem.set_mode(fid.path(), attr.mode())?;
        }

        if attr.has_atime() {
            if attr.has_atime_set() {
                self.filesystem.touch(fid.path(), FsTouch::Atime, attr.atime())?;
            } else {
                self.filesystem.touch(fid.path(), FsTouch::AtimeNow, (0,0))?;
            }
        }

        if attr.has_mtime() {
            if attr.has_mtime_set() {
                self.filesystem.touch(fid.path(), FsTouch::Mtime, attr.mtime())?;
            } else {
                self.filesystem.touch(fid.path(), FsTouch::MtimeNow, (0,0))?;
            }
        }

        if attr.has_chown() {
            let (uid, gid) = attr.chown_ids();
            self.filesystem.chown(fid.path(), uid, gid)?;
        }

        if attr.has_size() {
            self.filesystem.truncate(fid.path(), attr.size())?;
        }
        pp.write_done()
    }

    fn p9_readdir_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u64, u32)> {
        let fid = self.read_fid(pp)?;
        let offset = pp.r64()?;
        let count = pp.r32()?;
        pp.read_done()?;
        Ok((fid, offset, count))
    }

    fn p9_readdir(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, offset, count) = self.p9_readdir_args(pp)?;

        if self.debug {
            notify!("p9_readdir({}, offset={}, count={})", fid, offset, count);
        }

        if offset == 0 {
            fid.load_directory()?;
        }

        let mut dref = fid.directory();
        let directory = match dref.as_mut() {
            Some(directory) => directory,
            None => return system_error(libc::EBADF),
        };

        let size= cmp::min(self.msize - 4, count) as usize;
        directory.write_entries(pp, offset, size)?;
        pp.write_done()
    }

    fn p9_fsync_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u32)> {
        let fid = self.read_fid(pp)?;
        let datasync = pp.r32()?;
        pp.read_done()?;
        Ok((fid, datasync))
    }

    fn p9_fsync(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, datasync) = self.p9_fsync_args(pp)?;

        if self.debug {
            notify!("p9_fsync({}, {})", fid, datasync);
        }

        let file = fid.file()?;
        if datasync == 0 {
            file.sync_all()?;
        } else {
            file.sync_data()?;
        }
        pp.write_done()
    }

    fn p9_lock_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>,u8,u32)> {
        let fid = self.read_fid(pp)?;
        let ltype = pp.r8()?;
        let flags = pp.r32()?;
        let _ = pp.r64()?;
        let _ = pp.r64()?;
        let _ = pp.r32()?;
        let _ = pp.read_string()?;
        pp.read_done()?;
        Ok((fid, ltype, flags))
    }

    fn p9_lock(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, ltype,flags)= self.p9_lock_args(pp)?;
        if flags & !P9_LOCK_FLAGS_BLOCK != 0 {
            return system_error(libc::EINVAL);
        }
        let file = fid.file()?;
        let status = file.flock(ltype)?;
        pp.w8(status)?;
        pp.write_done()
    }

    fn p9_getlock_args(&mut self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u8, u64, u64, u32, String)> {
        let fid = self.read_fid(pp)?;
        let ltype = pp.r8()?;
        let start = pp.r64()?;
        let length= pp.r64()?;
        let pid = pp.r32()?;
        let client_id = pp.read_string()?;
        pp.read_done()?;

        Ok((fid, ltype, start, length, pid, client_id))
    }

    fn p9_getlock(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, ltype,start,length, pid, client_id) = self.p9_getlock_args(pp)?;

        let file = fid.file()?;
        let rtype = file.getlock(ltype)?;
        pp.w8(rtype)?;
        pp.w64(start)?;
        pp.w64(length)?;
        pp.w32(pid)?;
        pp.write_string(&client_id)?;
        pp.write_done()
    }

    fn p9_unlinkat_args(&self, pp: &mut PduParser) -> io::Result<(PathBuf, u32)> {
        let path = self.read_new_path(pp)?;
        let flags = pp.r32()?;
        pp.read_done()?;
        Ok((path, flags))
    }

    fn p9_unlinkat(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (path, flags) = self.p9_unlinkat_args(pp)?;

        if self.debug {
            notify!("p9_unlinkat({:?}, {:08x})", path, flags);
        }

        if path.is_dir() && (flags & libc::AT_REMOVEDIR as u32) == 0 {
            return system_error(libc::EISDIR);
        } else if path.is_dir() {
            self.filesystem.remove_dir(&path)?;
        } else {
            self.filesystem.remove_file(&path)?;
        }
        pp.write_done()
    }

    fn p9_link_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, PathBuf)> {
        let dfid = self.read_fid(pp)?;
        let fid = self.read_fid(pp)?;
        let name = pp.read_string()?;
        pp.read_done()?;
        let newpath = dfid.join_name(&self.root, &name)?;
        Ok((fid, newpath))
    }

    fn p9_link(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, newpath) = self.p9_link_args(pp)?;
        self.filesystem.link(fid.path(), &newpath)?;
        pp.write_done()
    }

    fn p9_mkdir_args(&self, pp: &mut PduParser) -> io::Result<(PathBuf, u32)> {
        let newpath = self.read_new_path(pp)?;
        let mode = pp.r32()?;
        let _gid = pp.r32()?;
        pp.read_done()?;
        Ok((newpath, mode))
    }

    fn p9_mkdir(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (newpath, mode) = self.p9_mkdir_args(pp)?;

        self.filesystem.create_dir(&newpath, mode)?;

        let qid = self.filesystem.read_qid(&newpath)?;
        qid.write(pp)?;

        pp.write_done()
    }

    fn p9_renameat_args(&self, pp: &mut PduParser) -> io::Result<(PathBuf, PathBuf)> {
        let oldpath = self.read_new_path(pp)?;
        let newpath = self.read_new_path(pp)?;
        pp.read_done()?;
        Ok((oldpath, newpath))
    }

    fn p9_renameat(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (oldpath, newpath) = self.p9_renameat_args(pp)?;
        self.filesystem.rename(&oldpath, &newpath)?;
        pp.write_done()?;
        Ok(())
    }

    fn p9_version_args(&self, pp: &mut PduParser) -> io::Result<(u32, String)> {
        let msize = pp.r32()?;
        let version = pp.read_string()?;
        pp.read_done()?;
        Ok((msize, version))
    }

    fn p9_version(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (msize, version) = self.p9_version_args(pp)?;

        if self.debug {
            notify!("p9_version({}, {})", version, msize);
        }

        self.msize = msize;
        self.fids.clear();

        pp.w32(msize)?;
        if version.as_str() == "9P2000.L" {
            pp.write_string(&version)?;
        } else {
            pp.write_string("unknown")?;
        }
        pp.write_done()
    }

    fn p9_attach_args(&self, pp: &mut PduParser) -> io::Result<u32> {
        let id = pp.r32()?;
        let _afid = pp.r32()?;
        let _uname = pp.read_string()?;
        let _aname = pp.read_string()?;
        let _uid = pp.r32()?;
        pp.read_done()?;
        Ok(id)
    }

    fn p9_attach(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let id = self.p9_attach_args(pp)?;

        if self.fids.exists(id) {
            return system_error(libc::EBADF);
        }

        let fid = self.fids.create(id, &self.root)?;
        fid.write_qid(pp)?;
        self.fids.add(fid);
        pp.write_done()
    }

    fn p9_flush(&mut self, pp: &mut PduParser) -> io::Result<()> {
        pp.read_done()?;
        pp.write_done()
    }

    fn p9_walk_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u32, Vec<String>)> {
        let fid = self.read_fid(pp)?;
        let newfid_id = pp.r32()?;
        let names = pp.read_string_list()?;
        pp.read_done()?;
        Ok((fid, newfid_id, names))
    }

    fn p9_walk(&mut self, pp: &mut PduParser) -> io::Result<()> {
        fn walk_extend<T: FileSystemOps>(fids: &Fids<T>, qid: Qid, path: &Path, name: &str) -> io::Result<(PathBuf, Qid)> {
            let path = fids.path_join_name(qid, path, name)?;
            let qid = fids.read_qid(&path)?;
            Ok((path, qid))
        }

        let (fid, newfid_id, names) = self.p9_walk_args(pp)?;

        if fid.id() != newfid_id && self.fids.exists(newfid_id) {
            return system_error(libc::EBADF);
        }

        if self.debug {
            notify!("p9_walk({}, newfid={}, names={:?})", fid, newfid_id, names);
        }

        let mut path = fid.path().to_path_buf();
        let mut current_qid = fid.qid();

        let mut qid_list = Vec::new();

        for name in names {
            path = match walk_extend(&self.fids, current_qid, &path, &name) {
                Ok((path, qid)) => {
                    qid_list.push(qid);
                    current_qid = qid;
                    path
                },
                Err(e) => {
                    if qid_list.is_empty() {
                        return Err(e);
                    }
                    pp.write_qid_list(&qid_list)?;
                    pp.write_done()?;
                    return Ok(())
                }
            };
        }

        let new_fid = self.fids.create(newfid_id, path)?;
        self.fids.add(new_fid);

        pp.write_qid_list(&qid_list)?;
        pp.write_done()
    }

    fn p9_read_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u64, u32)> {
        let fid = self.read_fid(pp)?;
        let offset = pp.r64()?;
        let count = pp.r32()?;
        pp.read_done()?;
        Ok((fid, offset, count))
    }

    fn p9_read(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, offset, count) = self.p9_read_args(pp)?;

        if self.debug {
            notify!("p9_read({}, offset={}, count={})", fid, offset, count);
        }

        let file = fid.file()?;
        // space for size field
        pp.w32(0)?;

        let mut nread = 0;

        while nread < count {
            let current = pp.chain.current_write_slice();
            if current.len() == 0 {
                break;
            }
            let rlen = cmp::min(current.len(), count as usize);
            let n = file.read_at(&mut current[..rlen], offset + nread as u64)?;
            if n == 0 {
                break;
            }
            pp.chain.inc_write_offset(n);
            nread += n as u32;
        }
        pp.w32_at(0, nread as u32);
        pp.write_done()
    }

    fn p9_write_args(&self, pp: &mut PduParser) -> io::Result<(&Fid<T>, u64, u32)> {
        let fid = self.read_fid(pp)?;
        let offset = pp.r64()?;
        let count = pp.r32()?;
        Ok((fid, offset, count))
    }

    fn p9_write(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let (fid, offset, count) = self.p9_write_args(pp)?;

        if self.debug {
            notify!("p9_write({}, offset={}, count={})", fid, offset, count);
        }

        let file = fid.file()?;
        let mut nread = 0;
        while nread < count {
            let n = file.write_at(pp.chain.current_read_slice(), offset + nread as u64)?;
            if n == 0 {
                break;
            }
            pp.chain.inc_read_offset(n);
            nread += n as u32;
        }
        pp.read_done()?;
        pp.w32(nread)?;
        pp.write_done()
    }

    fn remove_fid(&mut self, pp: &mut PduParser) -> io::Result<Fid<T>> {
        let id = pp.r32()?;
        pp.read_done()?;
        self.fids.remove(id)
    }

    fn p9_clunk(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = self.remove_fid(pp)?;
        if self.debug {
            notify!("p9_clunk({})", fid);
        }
        pp.write_done()
    }

    fn p9_remove(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = self.remove_fid(pp)?;
        if self.debug {
            notify!("p9_remove({})", fid);
        }
        if fid.is_dir() {
            self.filesystem.remove_dir(fid.path())?;
        } else {
            self.filesystem.remove_file(fid.path())?;
        }
        pp.write_done()
    }

    fn p9_unsupported(&self, pp: &mut PduParser) -> io::Result<()> {
        pp.read_done()?;
        system_error(libc::EOPNOTSUPP)
    }
}

