
use std::path::PathBuf;
use std::io;
use std::path::Path;
use std::fs;

use libc;

use memory::GuestRam;
use super::pdu::{PduParser,P9Attr};
use super::fid::FidCache;
use super::filesystem::{FileSystem,FsTouch,FileSystemOps};



const P9_TSTATFS: u8      = 8;
const P9_TLOPEN: u8       = 12;
const P9_TLCREATE: u8     = 14;
const P9_TSYMLINK: u8     = 16;
//const P9_TMKNOD: u8       = 18;
//const P9_TRENAME: u8      = 20;
const P9_TREADLINK: u8    = 22;
const P9_TGETATTR: u8     = 24;
const P9_TSETATTR: u8     = 26;
const P9_TXATTRWALK: u8   = 30;
const P9_TXATTRCREATE: u8 = 32;
const P9_TREADDIR: u8     = 40;
const P9_TFSYNC: u8       = 50;
const P9_TLOCK: u8        = 52;
const P9_TGETLOCK: u8     = 54;
//const P9_TLINK: u8        = 70;
//const P9_TMKDIR: u8       = 72;
//const P9_TRENAMEAT: u8    = 74;
//const P9_TUNLINKAT: u8    = 76;
const P9_TVERSION:u8      = 100;
const P9_TATTACH :u8      = 104;
//const P9_TFLUSH: u8       = 108;
const P9_TWALK :u8        = 110;
const P9_TREAD: u8        = 116;
//const P9_TWRITE: u8       = 118;
const P9_TCLUNK: u8       = 120;
//const P9_REMOVE: u8       = 122;

const P9_LOCK_SUCCESS:u32 = 0;
const F_UNLCK: u8 = 2;
const P9_VERSION_DOTL:&str = "9P2000.L";

pub struct Commands {
    filesystem: FileSystem,
    fids: FidCache,
    root_dir: PathBuf,
    _memory: GuestRam,
}

impl Commands {
    pub fn new(root_dir: PathBuf, init_path: PathBuf, memory: GuestRam) -> Commands {
        let  fsys = FileSystem::new(root_dir.clone(), init_path,true);
        Commands {
            filesystem: fsys.clone(),
            fids: FidCache::new(fsys.clone()),
            root_dir, _memory: memory,
        }
    }

    fn handle_io_result(&self, cmd: u8, result: io::Result<()>) {
        match result {
            Ok(()) => (),
            Err(e) => println!("io error in 9p command {} processing: {:?}",cmd, e),
        }
    }

    pub fn handle(&mut self, pp: &mut PduParser) {
        match pp.command() {
            Ok(cmd) => {
                let res = self.dispatch(cmd, pp);
                self.handle_io_result(cmd,res);
            },
            Err(e) => self.handle_io_result(0,Err(e)),
        }
    }

    fn dispatch(&mut self, cmd: u8, pp: &mut PduParser) -> io::Result<()> {
        match cmd {
            P9_TSTATFS => self.p9_statfs(pp)?,
            P9_TLOPEN => self.p9_open(pp)?,
            P9_TLCREATE => self.p9_create(pp)?,
            P9_TSYMLINK => self.p9_symlink(pp)?,
            //P9_TMKNOD => self.p9_mknod(pp)?,
            //P9_TRENAME => self.p9_rename(pp)?,
            P9_TREADLINK => self.p9_readlink(pp)?,
            P9_TGETATTR => self.p9_getattr(pp)?,
            P9_TSETATTR => self.p9_setattr(pp)?,
            P9_TXATTRWALK => self.p9_unsupported(pp)?,
            P9_TXATTRCREATE =>  self.p9_unsupported(pp)?,
            P9_TREADDIR => self.p9_readdir(pp)?,
            P9_TFSYNC => self.p9_fsync(pp)?,
            P9_TLOCK => self.p9_lock(pp)?,
            P9_TGETLOCK => self.p9_getlock(pp)?,
            //P9_TLINK => self.p9_link(pp)?,
            //P9_TMKDIR=> self.p9_mkdir(pp)?,
            //P9_TRENAMEAT => self.p9_renameat(pp)?,
            //P9_UNLINKAT => self.p9_unlinkat(pp)?,
            P9_TVERSION => self.p9_version(pp)?,
            P9_TATTACH => self.p9_attach(pp)?,
            //P9_FLUSH => self.p9_flush(pp)?,
            P9_TWALK => self.p9_walk(pp)?,
            P9_TREAD => self.p9_read(pp)?,
            //P9_WRITE => self.p9_write(pp)?,
            P9_TCLUNK => self.p9_clunk(pp)?,
            //P9_REMOVE => self.p9_remove(pp)?,
            n => println!("unhandled 9p command: {}", n),
        }
        Ok(())
    }

    fn p9_unsupported(&self, pp: &mut PduParser) -> io::Result<()> {
        pp.read_done()?;
        pp.bail_err(io::Error::from_raw_os_error(libc::EOPNOTSUPP))

    }

    fn p9_statfs(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = pp.r32()?;
        pp.read_done()?;
        match self.fids.statfs(fid) {
            Ok(statfs) => {
                pp.write_statfs(statfs)?;
                pp.write_done()
            },
            Err(err) => pp.bail_err(err),
        }
    }

    fn p9_version(&self, pp: &mut PduParser) -> io::Result<()> {
        let msize = pp.r32()?;
        let version = pp.read_string()?;
        pp.read_done()?;

        pp.w32(msize)?;
        if version == P9_VERSION_DOTL {
            pp.write_string(&version)?;
        } else {
            pp.write_string("unknown")?;
        }

        pp.write_done()
    }

    fn p9_attach(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid_val = pp.r32()?;
        let _afid = pp.r32()?;
        let _uname = pp.read_string()?;
        let _aname = pp.read_string()?;
        let uid = pp.r32()?;
        pp.read_done()?;

        self.fids.with_fid_mut(fid_val, |fid| {
            fid.uid = uid;
            fid.path.push("/");
        });

        match fs::metadata(&self.root_dir) {
            Ok(ref meta) => {
                pp.write_qid(meta)?;
                pp.write_done()
            }
            Err(e) => pp.bail_err(e),
        }
    }

    fn p9_open(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = pp.r32()?;
        let flags = pp.r32()?;
        pp.read_done()?;

        if let Err(err) = self.fids.open(fid, flags) {
            return pp.bail_err(err);
        }

        let meta = match self.fids.metadata(fid) {
            Ok(meta) => meta,
            Err(err) => {
                return pp.bail_err(err);
            }
        };

        pp.write_qid(&meta)?;
        // XXX iounit goes here
        pp.w32(0)?;
        pp.write_done()
    }

    fn p9_create(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let dfid = pp.r32()?;
        let name = pp.read_string()?;
        let flags = pp.r32()?;
        let mode = pp.r32()?;
        let gid = pp.r32()?;
        pp.read_done()?;

        match self.fids.create(dfid, name, flags, mode, gid) {
            Ok(meta) => {
                pp.write_statl(&meta)?;
                pp.write_done()?;

            },
            Err(err) => return pp.bail_err(err),
        }
        Ok(())
    }

    fn p9_symlink(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let _fid = pp.r32()?;
        let _name = pp.read_string()?;
        let _old_path = pp.read_string()?;
        let _gid = pp.r32()?;
        pp.read_done()?;
        // XXX
        pp.write_done()
    }

    fn p9_read(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let id = pp.r32()?;
        let off = pp.r64()?;
        let cnt = pp.r32()?;
        pp.read_done()?;

        // space for size field
        pp.w32(0)?;

        match self.fids.fid_mut(id).read(off, cnt as usize, pp) {
            Ok(nread) => {
                // write nread in space reserved earlier
                pp.w32_at(0, nread as u32);
                pp.write_done()?;
            }
            Err(err) => {
                println!("oops error on read: {:?}", err);
                return pp.bail_err(err)
            },
        };
        Ok(())
    }

    fn p9_readdir(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let id = pp.r32()?;
        let off = pp.r64()?;
        let cnt = pp.r32()?;
        pp.read_done()?;

        self.fids.readdir(id,off, cnt as usize, pp)
    }

    fn p9_clunk(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let id = pp.r32()?;
        pp.read_done()?;
        self.fids.clunk(id);
        pp.write_done()
    }

    fn p9_readlink(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let id = pp.r32()?;
        pp.read_done()?;
        let link = self.fids.readlink(id)?;
        pp.write_os_string(&link)?;
        pp.write_done()
    }

    fn p9_getattr(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let id = pp.r32()?;
        let _mask = pp.r64()?;
        pp.read_done()?;

        let meta = match self.fids.metadata(id) {
            Ok(meta) => meta,
            Err(e) => return pp.bail_err(e),
        };

        pp.write_statl(&meta)?;
        pp.write_done()
    }

    fn do_setattr(&mut self, fid: u32, attr: P9Attr) -> io::Result<()> {
        if attr.has_mode() {
            self.fids.chmod(fid, attr.mode())?
        }
        if attr.has_atime() {
            if attr.has_atime_set() {
                self.fids.touch(fid, FsTouch::Atime,attr.atime())?
            } else {
                self.fids.touch(fid, FsTouch::AtimeNow,(0,0))?
            }
        }

        if attr.has_mtime() {
            if attr.has_mtime_set() {
                self.fids.touch(fid, FsTouch::Mtime,attr.mtime())?
            } else {
                self.fids.touch(fid, FsTouch::MtimeNow,(0,0))?
            }
        }

        if attr.has_chown() {
            let (uid, gid) = attr.chown_ids();
            self.fids.chown(fid, uid, gid)?;
        }

        if attr.has_size() {
            self.fids.truncate(fid, attr.size())?;
        }

        Ok(())
    }

    fn p9_setattr(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = pp.r32()?;
        let attr = pp.read_attr()?;
        pp.read_done()?;

        if let Err(err) = self.do_setattr(fid, attr) {
            return pp.bail_err(err)
        }

        pp.write_done()
    }

    // XXX look at walk in qemu
    fn p9_walk(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid_id = pp.r32()?;
        let new_fid_id = pp.r32()?;
        let nwname = pp.r16()?;

        self.fids.dup_fid(fid_id, new_fid_id);

        let mut cur = self.fids.fid(new_fid_id).path.clone();
        let mut metalist = Vec::new();
        for _ in 0..nwname {
            let s = pp.read_string()?;
            let p = Path::new(&s);
            if p.components().count() != 1 {
                println!("uh...");
            }
            cur.push(p);
            match self.filesystem.stat(&cur) {
                Ok(m) => metalist.push(m),
                Err(e) => {
                    pp.read_done()?;
                    return pp.bail_err(e)
                },
            }
        }
        self.fids.with_fid_mut(new_fid_id, |fid| {
            fid.path = cur;
        });

        pp.read_done()?;
        pp.w16(metalist.len() as u16)?;
        for meta in metalist {
            pp.write_qid(&meta)?;
        }
        pp.write_done()
    }

    fn p9_fsync(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let fid = pp.r32()?;
        let dsync = pp.r32()?;
        pp.read_done()?;
        if let Err(err) = self.fids.fsync(fid, dsync != 0) {
           return pp.bail_err(err);
        }
        pp.write_done()
    }

    fn p9_lock(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let _ = pp.r32()?;
        let _ = pp.r8()?;
        let _ = pp.r32()?;
        let _ = pp.r64()?;
        let _ = pp.r64()?;
        let _ = pp.r32()?;
        let _ = pp.read_string()?;
        pp.read_done()?;

        pp.w32(P9_LOCK_SUCCESS)?;
        pp.write_done()
    }

    fn p9_getlock(&mut self, pp: &mut PduParser) -> io::Result<()> {
        let _fid = pp.r32()?;
        let _type = pp.r8()?;
        let glock_start = pp.r64()?;
        let glock_len = pp.r64()?;
        let glock_proc_id = pp.r32()?;
        let glock_client_id = pp.read_string()?;
        pp.read_done()?;

        pp.w8(F_UNLCK)?;
        pp.w64(glock_start)?;
        pp.w64(glock_len)?;
        pp.w32(glock_proc_id)?;
        pp.write_string(&glock_client_id)?;
        pp.write_done()
    }
}
