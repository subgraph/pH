use std::os::unix::io::{IntoRawFd,AsRawFd,RawFd};
use std::{mem, io};
use crate::system::errno::cvt;
use std::os::raw::c_void;
use libc::c_int;
use std::io::SeekFrom;

#[derive(Debug)]
pub struct FileDesc {
    fd: RawFd,
}

#[derive(Copy,Clone,Debug,Eq,PartialEq)]
pub enum FileFlags {
    Read,
    Write,
    ReadWrite,
}

#[allow(dead_code)]
impl FileDesc {
    pub fn new(fd: RawFd) -> Self {
        FileDesc { fd }
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let ret = cvt(unsafe {
            libc::read(self.fd, buf.as_mut_ptr() as *mut c_void, buf.len())
        })?;
        Ok(ret as usize)
    }

    pub fn read_exact(&mut self, mut buf: &mut [u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => break,
                Ok(n) => { let tmp = buf; buf = &mut tmp[n..]; }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if !buf.is_empty() {
            Err(io::Error::new(io::ErrorKind::UnexpectedEof,
                           "failed to fill whole buffer"))
        } else {
            Ok(())
        }
    }
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        unsafe {
            let v = nonblocking as c_int;
            cvt(libc::ioctl(self.fd, libc::FIONBIO, &v))?;
            Ok(())
        }
    }
    pub fn set_cloexec(&self) -> io::Result<()> {
        unsafe {
            cvt(libc::ioctl(self.fd, libc::FIOCLEX))?;
            Ok(())
        }
    }

    pub fn flags(&self) -> io::Result<FileFlags> {
        let flags = unsafe { cvt(libc::fcntl(self.fd, libc::F_GETFL))? };
        match flags & libc::O_ACCMODE {
            libc::O_RDONLY => Ok(FileFlags::Read),
            libc::O_WRONLY => Ok(FileFlags::Write),
            libc::O_RDWR => Ok(FileFlags::ReadWrite),
            _ => Err(io::Error::from_raw_os_error(libc::EINVAL)),
        }
    }

    pub fn seek(&self, pos: SeekFrom) -> io::Result<u64> {
        let (whence, pos) = match pos {
            // Casting to `i64` is fine, too large values will end up as
            // negative which will cause an error in `lseek64`.
            SeekFrom::Start(off) => (libc::SEEK_SET, off as i64),
            SeekFrom::End(off) => (libc::SEEK_END, off),
            SeekFrom::Current(off) => (libc::SEEK_CUR, off),
        };
        let n = cvt(unsafe { libc::lseek64(self.fd, pos, whence) })?;
        Ok(n as u64)
    }
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let ret = cvt(unsafe {
            libc::write(self.fd,
                        buf.as_ptr() as *const c_void,
                        buf.len())
        })?;
        Ok(ret as usize)
    }

    pub fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut buf = buf;
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                   "failed to write whole buffer")),
                Ok(n) => {
                    buf = &buf[n..];
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())


    }

    pub fn set_size(&self, size: usize) -> io::Result<()> {
        unsafe {
            if libc::ftruncate64(self.fd, size as libc::off64_t) < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    pub fn flush(&self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for FileDesc {
    fn drop(&mut self) {
        let _ = unsafe { libc::close(self.fd) };
    }
}
impl AsRawFd for FileDesc {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl IntoRawFd for FileDesc {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.fd;
        mem::forget(self);
        fd
    }
}