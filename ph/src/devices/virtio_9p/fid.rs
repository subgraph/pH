use std::fs::Metadata;
use std::collections::HashMap;
use std::path::PathBuf;
use std::io::{self, Seek,Write};
use std::os::unix::io::AsRawFd;
use std::ffi::OsString;

use libc;
use super::pdu::PduParser;
use super::readdir::DirEntry;
use super::filesystem::{FileSystem,FileDescriptor,StatFs,FileSystemOps,FsTouch};

pub struct FidCache {
    filesystem: FileSystem,
    fidmap: HashMap<u32, Fid>,
}

impl FidCache {
    pub fn new(filesystem: FileSystem) -> FidCache {
        FidCache {
            filesystem,
            fidmap: HashMap::new(),
        }
    }

    fn add_if_absent(&mut self, id: u32) {
        if !self.fidmap.contains_key(&id) {
            self.fidmap.insert(id, Fid::new());
        }
    }

    pub fn fid(&mut self, id: u32) -> &Fid {
        self.add_if_absent(id);
        self.fidmap.get(&id).expect("fidmap does not have element")
    }

    pub fn _fid(&self, id: u32) -> &Fid {
        self.fidmap.get(&id).expect("fidmap does not have element")
    }

    pub fn fid_mut(&mut self, id: u32) -> &mut Fid {
        self.add_if_absent(id);
        self.fidmap.get_mut(&id).expect("fidmap does not have element")
    }

    pub fn with_fid_mut<F,U>(&mut self, id: u32, f: F) -> U
        where F: FnOnce(&mut Fid) -> U {
        self.add_if_absent(id);
        f(self.fid_mut(id))
    }

    #[allow(dead_code)]
    pub fn with_fid<F,U>(&mut self, id: u32, f: F) -> U
        where F: FnOnce(&Fid) -> U {
        self.add_if_absent(id);
        f(self.fid(id))
    }

    pub fn dup_fid(&mut self, old_id: u32, new_id: u32) {
        self.fid_mut(new_id).path = self.fid(old_id).path.clone();
        self.fid_mut(new_id).uid = self.fid(old_id).uid;
    }

    pub fn clunk(&mut self, id: u32) {
        match self.fidmap.remove(&id) {
            Some(ref mut fid) => fid.close(),
            None => (),
        }
    }

    pub fn open(&mut self, id: u32, flags: u32) -> io::Result<()> {
        let path = self.fid(id).path.clone();
        let fd = self.filesystem.open(&path, flags)?;
        self.fid_mut(id).desc = fd;
        Ok(())
    }

    fn fid_dir_join(&mut self, id: u32, name: &str) -> io::Result<PathBuf> {
        let meta = self.metadata(id)?;
        if !meta.is_dir() {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }

        let fname = PathBuf::from(name);
        if fname.is_absolute() || fname.components().count() != 1 {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let mut path = self.fid(id).path.clone();
        path.push(fname);
        Ok(path)
    }

    pub fn create(&mut self, id: u32, name: String, flags: u32, mode: u32, gid: u32) -> io::Result<Metadata> {
        let path = self.fid_dir_join(id,&name)?;

        self.filesystem.create(&path, flags, mode)?;

        let uid = self.fid(id).uid;
        self.filesystem.chown(&path, uid, gid)?;

        self.filesystem.stat(&path)
    }

    pub fn readlink(&mut self, id: u32) -> io::Result<OsString> {
        let path = self.fid(id).path.clone();
        self.filesystem.readlink(&path)
    }

    pub fn metadata(&mut self, id: u32) -> io::Result<Metadata> {
        let path = self.fid(id).path.clone();
        self.filesystem.stat(&path)
    }

    pub fn readdir(&mut self, id: u32, off: u64, len: usize, pp: &mut PduParser) -> io::Result<()> {
        //let is_dir = self.fid(id).desc.is_dir();
        if off != 0 {
            //self.fid_mut(id).desc.borrow_dir().unwrap().seek(off as i64);
        }
        self.fid_mut(id).readdir(len, pp)
    }

    pub fn chmod(&mut self, id: u32, mode: u32) -> io::Result<()> {
        let path = self.fid(id).path.clone();
        self.filesystem.chmod(&path, mode)
    }

    pub fn chown(&mut self, id: u32, uid: u32, gid: u32) -> io::Result<()> {
        let path = self.fid(id).path.clone();
        self.filesystem.chown(&path, uid, gid)
    }

    pub fn touch(&mut self, id: u32, which: FsTouch, tv: (u64,u64)) -> io::Result<()> {
        let path = self.fid(id).path.clone();
        self.filesystem.touch(&path, which, tv)
    }

    pub fn truncate(&mut self, _id: u32, _size: u64) -> io::Result<()> {
        Ok(())
    }

    pub fn statfs(&mut self, fid: u32) -> io::Result<StatFs> {
        let path = self.fid(fid).path.clone();
        self.filesystem.statfs(&path)
    }

    pub fn fsync(&mut self, fid: u32, datasync: bool) -> io::Result<()> {
        match self.fid(fid).desc {
            FileDescriptor::File(ref file) => {
                let fd = file.as_raw_fd();
                unsafe {
                    let res = if datasync {
                        libc::fdatasync(fd)
                    } else {
                        libc::fsync(fd)
                    };
                    if res < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
            },
            FileDescriptor::Dir(ref dir) => { return dir.fsync(); },
            FileDescriptor::None => { return Err(io::Error::from_raw_os_error(libc::EBADF))},
        };
        Ok(())

    }
}

pub struct Fid {
    pub uid: u32,
    pub path: PathBuf,
    desc: FileDescriptor,
}

impl Fid {
    fn new() -> Fid {
        Fid {
            uid: 0, path: PathBuf::new(), desc: FileDescriptor::None,
        }
    }

    pub fn read(&mut self, offset: u64, len: usize, pp: &mut PduParser) -> io::Result<(usize)> {
        self.desc.borrow_file()?.seek(io::SeekFrom::Start(offset))?;
        pp.chain.copy_from_reader(self.desc.borrow_file()?, len)
    }

    fn dirent_len(dent: &DirEntry) -> usize {
        // qid + offset + type + strlen + str
        return 13 + 8 + 1 + 2 + dent.name_bytes().len()
    }

    fn write_dirent(dent: &DirEntry, pp: &mut PduParser) -> io::Result<()> {
        pp.write_qid_path_only(dent.ino())?;
        pp.w64(dent.offset())?;
        pp.w8(dent.file_type())?;
        pp.w16(dent.name_bytes().len() as u16)?;
        pp.chain.write(&dent.name_bytes())?;
        Ok(())
    }

    pub fn readdir(&mut self, len: usize, pp: &mut PduParser) -> io::Result<()> {
        let mut write_len = 0_usize;
        pp.w32(0)?;


        while let Some(entry) = self.desc.borrow_dir()?.next() {
            match entry {
                Ok(ref dent) => {
                    let dlen = Fid::dirent_len(dent);
                    if write_len + dlen > len {
                        self.desc.borrow_dir()?.restore_last_pos();
                        break;
                    }
                    write_len += dlen;
                    Fid::write_dirent(dent, pp)?;
                }
                Err(err) => return pp.bail_err(err),
            }
        }


        pp.w32_at(0, write_len as u32);
        pp.write_done()
    }

    pub fn close(&mut self) {
        self.desc = FileDescriptor::None;
    }
}

