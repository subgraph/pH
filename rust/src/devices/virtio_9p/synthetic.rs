use std::collections::{HashSet, BTreeMap};
use std::collections::btree_map::Entry;
use std::ffi::{OsString, OsStr};
use std::io;
use std::os::linux::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf, Component};
use std::process::{Command, Stdio};
use std::time::{UNIX_EPOCH, SystemTime};

use crate::devices::virtio_9p::{
    directory::{Directory, P9DirEntry},
    file::{P9File, Qid, P9_QTDIR, P9_QTFILE},
    filesystem::{FileSystemOps, FsTouch, FileSystem},
    pdu::PduParser,
};
use crate::devices::virtio_9p::file::Buffer;

#[derive(Clone)]
struct NodeData {
    name: OsString,
    qid: Qid,
    size: u64,
    mode: u32,
    inode: u32,
}

impl NodeData {

    fn name_str(&self) -> &str {
        self.name.to_str()
            .expect("SyntheticFS: unable to convert name to &str")
    }

    fn dtype(&self) -> u8 {
        if self.qid.is_dir() {
            libc::DT_DIR
        } else {
            libc::DT_REG
        }
    }
}

#[derive(Clone)]
enum Node {
    File(PathBuf, NodeData),
    MemoryFile(Buffer<&'static [u8]>, NodeData),
    Dir(BTreeMap<OsString, Node>, NodeData),
}

impl Node {
    fn new_dir(name: &OsStr, mode: u32, inode: u32) -> Node {
        let mode = mode | libc::S_IFDIR;
        let data = NodeData::new(name, P9_QTDIR, 0, mode, inode);
        let entries= BTreeMap::new();
        Node::Dir(entries, data)
    }

    fn new_file<S: Into<OsString>>(name: S, mode: u32, inode: u32, size: u64, local: &Path) -> Node {
        let mode = mode | libc::S_IFREG;
        let data = NodeData::new(name, P9_QTFILE, size, mode, inode);
        let local = local.to_path_buf();
        Node::File(local, data)
    }

    fn new_memory_file<S: Into<OsString>>(name: S, mode: u32, inode: u32, size: u64, bytes: &'static [u8]) -> Node {
        let mode = mode | libc::S_IFREG;
        let data = NodeData::new(name, P9_QTFILE, size, mode, inode);
        let buffer = Buffer::new(bytes);
        Node::MemoryFile(buffer, data)
    }

    fn node_data(&self) -> &NodeData {
        match self {
            Node::Dir(_, data) => data,
            Node::File(_, data) => data,
            Node::MemoryFile(_, data) => data,
        }
    }
    fn qid(&self) -> Qid {
        self.node_data().qid
    }

    fn write_stat(&self, pp: &mut PduParser) -> io::Result<()> {
        self.node_data().write_stat(pp)
    }

    fn create_directory_entry(&self, offset: u64) -> P9DirEntry {
        let data = self.node_data();
        P9DirEntry::new(data.qid, offset, data.dtype(), data.name_str())
    }


    fn entries(&self) -> Option<&BTreeMap<OsString, Node>> {
        match self {
            Node::Dir(entries, ..) => Some(entries),
            _ => None,
        }
    }

    fn entries_mut(&mut self) -> Option<&mut BTreeMap<OsString, Node>> {
        match self {
            Node::Dir(entries, ..) => Some(entries),
            _ => None,
        }
    }

    fn descend(&self, name: &OsStr) -> io::Result<&Node> {
        self.entries()
            .ok_or(rawerr(libc::ENOTDIR))
            .and_then(|entries|
                entries.get(name)
                .ok_or(rawerr(libc::ENOENT)))
    }

    fn descend_mut(&mut self, name: &OsStr) -> io::Result<&mut Node> {
        self.entries_mut()
            .ok_or(rawerr(libc::ENOTDIR))
            .and_then(|entries|
                entries.get_mut(name)
                .ok_or(rawerr(libc::ENOENT)))

    }

    fn mkdir(&mut self, names: &[&OsStr], mode: u32, inodes: &mut Inodes) -> io::Result<()> {
        if !names.is_empty() {
            let entries = self.entries_mut().ok_or(rawerr(libc::ENOTDIR))?;
            match entries.entry(names[0].to_os_string()) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().mkdir(&names[1..], mode, inodes)?;
                }
                Entry::Vacant(entry) => {
                    let inode = inodes.next_inode();

                    let mut node = Node::new_dir(names[0], mode, inode);
                    node.mkdir(&names[1..], mode, inodes)?;
                    entry.insert(node);
                }
            }
        }
        Ok(())
    }

    fn populate_directory(&self) -> io::Result<Directory> {
        match self {
            Node::Dir(nodes, ..) => {
                let mut offset = 0;
                let mut directory = Directory::new();
                for  node in nodes.values() {
                    let entry = node.create_directory_entry(offset);
                    offset = entry.offset();
                    directory.push_entry(entry);
                }
                return Ok(directory)
            },
            _ => return Err(io::Error::from_raw_os_error(libc::ENOTDIR)),
        }
    }
}

impl NodeData {
    fn new<S: Into<OsString>>(name: S, qtype: u8, size: u64, mode: u32, inode: u32) -> Self {
        NodeData {
            name: name.into(),
            qid: Self::create_qid(qtype, inode),
            size, mode, inode,
        }
    }

    fn create_qid(qtype: u8, inode: u32) -> Qid {
        let qid_version = Self::generate_qid_version(inode);
        let qid_path = inode as u64;
        Qid::new(qtype, qid_version, qid_path)
    }

    fn generate_qid_version(inode: u32) -> u32 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(v) => v.as_millis() as u32,
            _ => inode,
        }
    }

    fn write_stat(&self, pp: &mut PduParser) -> io::Result<()> {
        const P9_STATS_BASIC: u64 =  0x000007ff;
        pp.w64(P9_STATS_BASIC)?;
        self.qid.write(pp)?;

        pp.w32(self.mode)?;
        pp.w32(0)?;   // uid
        pp.w32(0)?;   // gid
        pp.w64(1)?;   // nlink
        pp.w64(0)?;   // rdev
        pp.w64(self.size)?;  // size
        pp.w64(0)?;   // blksize
        pp.w64(0)?;   // blocks
        pp.w64(0)?;   // atime
        pp.w64(0)?;   // atime nsec
        pp.w64(0)?;   // mtime
        pp.w64(0)?;   // mtime nsec
        pp.w64(0)?;   // ctime
        pp.w64(0)?;   // ctime nsec
        pp.w64(0)?;   // btime
        pp.w64(0)?;   // btime nsec
        pp.w64(0)?;
        pp.w64(0)?;
        Ok(())
    }
}

const BASE_INODE: u32 = 1000;
#[derive(Clone)]
struct Inodes {
    inodes: HashSet<u32>,
    current_inode: u32,
}

impl Inodes {
    fn new() -> Self {
        Inodes {
            inodes: HashSet::new(),
            current_inode: BASE_INODE,
        }
    }

    fn next_inode(&mut self) -> u32 {
        let mut inode = self.current_inode;
        while self.inodes.contains(&inode) {
            inode += 1;
        }
        self.inodes.insert(inode);
        self.current_inode = inode + 1;
        inode
    }

    fn file_inode(&mut self, path: &Path) -> u32 {
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(_) => return self.next_inode(),
        };

        let inode = meta.st_ino() as u32;
        if self.inodes.contains(&inode) {
            return self.next_inode();
        }
        self.inodes.insert(inode);
        inode
    }
}

#[derive(Clone)]
pub struct SyntheticFS {
    paths_added: HashSet<PathBuf>,
    root: Node,
    inodes: Inodes,
    euid_root: bool,
}
impl SyntheticFS {

    pub fn new() -> Self {
        let mut inodes = Inodes::new();
        let root = Node::new_dir("/".as_ref(), 0o755, inodes.next_inode());
        let euid_root = FileSystem::is_euid_root();

        SyntheticFS {
            root, inodes, euid_root, paths_added: HashSet::new(),
        }
    }

    fn node_count(&self) -> usize {
        self.inodes.inodes.len()
    }

    pub fn mkdirs<P: AsRef<Path>>(&mut self, paths: &[P]) {
        for p in paths {
            self.mkdir(p, 0o755);
        }
    }

    pub fn mkdir<P: AsRef<Path>>(&mut self, path: P, mode: u32) {
        let path = path.as_ref();
        let names = match Self::path_names(path) {
            Ok(names) => names,
            Err(_) => {
                warn!("cannot add directory because path is invalid: {}", path.display());
                return;
            }
        };
        if let Err(e) = self.root.mkdir(&names, mode, &mut self.inodes) {
            warn!("failed to create directory {}: {}", path.display(), e);
        }
    }

    #[allow(dead_code)]
    pub fn add_memory_file<S: Into<OsString>, P: AsRef<Path>>(&mut self, dirpath: P, filename: S, mode: u32, bytes: &'static [u8]) -> io::Result<()> {
        let dirpath = dirpath.as_ref();
        let filename = filename.into();
        self.mkdir(dirpath, 0o755);
        let inode = self.inodes.next_inode();
        let node = self.lookup_mut(dirpath)?;
        let entries = node.entries_mut().ok_or(rawerr(libc::ENOTDIR))?;
        entries.insert(OsString::from(filename.clone()), Node::new_memory_file(filename, mode, inode, bytes.len() as u64, bytes));
        Ok(())

    }
    pub fn add_file<S: Into<OsString>, P: AsRef<Path>, Q: AsRef<Path>>(&mut self, dirpath: P, filename: S, mode: u32, realpath: Q) {
        let dirpath = dirpath.as_ref();
        let realpath = realpath.as_ref();
        let filename = filename.into();
        if let Err(e) = self._add_file(dirpath, &filename, mode, realpath) {
            warn!("error adding file {:?} to {}: {}", filename, dirpath.display(), e);
        }
    }

    pub fn _add_file<S: Into<OsString>>(&mut self, dirpath: &Path, filename: S, mode: u32, realpath: &Path) -> io::Result<()> {
        let filename = filename.into();
        self.mkdir(dirpath, 0o755);
        let inode = self.inodes.file_inode(realpath);
        let node = self.lookup_mut(dirpath)?;
        let entries = node.entries_mut().ok_or(rawerr(libc::ENOTDIR))?;
        let meta = realpath.metadata()?;
        entries.insert(OsString::from(filename.clone()), Node::new_file(filename, mode, inode, meta.len(), realpath));
        Ok(())
    }

    fn parse_ldd_line(line: &str) -> Option<PathBuf> {
        for s in line.split_whitespace().take(3) {
            if s.starts_with('/') {
                let path = Path::new(s);
                if path.exists() {
                    return Some(path.to_path_buf())
                }
            }
        }
        None
    }

    fn add_path(&mut self, path: &Path) -> io::Result<()> {
        if let (Some(parent), Some(filename)) = (path.parent(), path.file_name()) {
            let meta = path.metadata()?;
            let mode = meta.permissions().mode();
            self.add_file(parent, filename, mode, path);
        }
        Ok(())
    }

    fn ldd_command() -> io::Result<Command> {
        let ldd = Path::new("/usr/bin/ldd");
        let ldso = Path::new("/usr/lib/ld-linux-x86-64.so.2");

        if ldd.exists() {
            Ok(Command::new(ldd))
        } else if ldso.exists() {
            let mut cmd = Command::new(ldso);
            cmd.arg("--list");
            Ok(cmd)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "No ldd binary found"))
        }
    }

    pub fn add_library_dependencies<P: AsRef<Path>>(&mut self, execpath: P) -> io::Result<()> {
        let execpath = execpath.as_ref();
        let mut cmd = Self::ldd_command()?;
        let out = cmd
            .arg(execpath.as_os_str())
            .stdout(Stdio::piped())
            .output()?;
        let s = String::from_utf8(out.stdout).expect("");

        for line in s.lines() {
            if let Some(path) = Self::parse_ldd_line(line) {
                if !self.paths_added.contains(&path) {
                    self.add_path(&path)?;
                    self.paths_added.insert(path);
                }
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn add_executable<P: AsRef<Path>, Q: AsRef<Path>>(&mut self, dirpath: P, filename: &str, realpath: Q) -> io::Result<()> {
        let realpath = realpath.as_ref();
        self.add_library_dependencies(realpath)?;

        self.add_file(dirpath, filename, 0o755, realpath);
        Ok(())
    }

    fn lookup(&self, path: &Path) -> io::Result<&Node> {
        let mut current = &self.root;
        for name in Self::path_names(path)? {
            current = current.descend(name)?;
        }
        Ok(current)
    }

    fn lookup_mut(&mut self, path: &Path) -> io::Result<&mut Node> {
        let mut current = &mut self.root;
        for name in Self::path_names(path)? {
            current = current.descend_mut(name)?;
        }
        Ok(current)
    }

    fn path_names(path: &Path) -> io::Result<Vec<&OsStr>> {
        if !path.is_absolute() {
            return syserr(libc::EINVAL)
        }
        Ok(path.components().flat_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        }).collect())
    }
}

impl FileSystemOps for SyntheticFS {
    fn read_qid(&self, path: &Path) -> io::Result<Qid> {
        let node = self.lookup(path)?;
        Ok(node.qid())
    }

    fn write_stat(&self, path: &Path, pp: &mut PduParser) -> io::Result<()> {
        let node = self.lookup(path)?;
        node.write_stat(pp)
    }

    fn open(&self, path: &Path, flags: u32) -> io::Result<P9File> {
        match self.lookup(path)? {
            Node::File(local, _) => {
                // XXX filter flags
                let file = FileSystem::open_with_flags(local, flags, self.euid_root)?;
                Ok(P9File::from_file(file))
            },
            Node::Dir(..) => {
                Ok(P9File::new_not_a_file())
            },
            Node::MemoryFile(buffer,..) => {
                Ok(P9File::from_buffer(buffer.clone()))
            }
        }
    }

    fn create(&self, _path: &Path, _flags: u32, _mode: u32) -> io::Result<P9File> {
        syserr(libc::EROFS)
    }

    fn write_statfs(&self, _path: &Path, pp: &mut PduParser) -> io::Result<()> {
        //notify!("write_statfs({})", path.display());
        let f_files = self.node_count() as u64;
        pp.w32(0xABCD)?;  // f_type
        pp.w32(512)?;     // f_bsize
        pp.w64(0)?;       // f_blocks
        pp.w64(0)?;       // f_bfree
        pp.w64(0)?;       // f_bavail
        pp.w64(f_files)?; // f_files
        pp.w64(0)?;       // f_ffree
        pp.w64(0)?;       //
        pp.w32(4096)?;    // f_namelen
        Ok(())
    }

    fn chown(&self, _path: &Path, _uid: u32, _gid: u32) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn set_mode(&self, _path: &Path, _mode: u32) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn touch(&self, _path: &Path, _which: FsTouch, _tv: (u64, u64)) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn truncate(&self, _path: &Path, _size: u64) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn readlink(&self, _path: &Path) -> io::Result<OsString> {
        syserr(libc::EROFS)
    }

    fn symlink(&self, _target: &Path, _linkpath: &Path) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn link(&self, _target: &Path, _newpath: &Path) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn rename(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn remove_file(&self, _path: &Path) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn remove_dir(&self, _path: &Path) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn create_dir(&self, _path: &Path, _mode: u32) -> io::Result<()> {
        syserr(libc::EROFS)
    }

    fn readdir_populate(&self, path: &Path) -> io::Result<Directory> {
        let node = self.lookup(path)?;
        node.populate_directory()
    }
}

fn rawerr(errno: i32) -> io::Error {
    io::Error::from_raw_os_error(errno)
}
fn syserr<T>(errno: i32) -> io::Result<T> {
    Err(rawerr(errno))
}