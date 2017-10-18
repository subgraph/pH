use libc;
use std::ptr;
use std::slice;
use std::mem;
use std::io::Write;
use std::os::unix::io::RawFd;

use vm::{Result,Error,ErrorKind};

pub struct Mapping {
    ptr: *mut u8,
    size: usize,
}

/// Marks types that can be passed to `write_int` and returned from `read_int`
pub unsafe trait Serializable: Copy+Send+Sync {}

unsafe impl Serializable for u8 {}
unsafe impl Serializable for u16 {}
unsafe impl Serializable for u32 {}
unsafe impl Serializable for u64 {}

unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

/// A block of memory returned from the mmap() system call.  Provides safe access to the raw
/// memory region.
impl Mapping {

    /// Creates a new anonymous mapping of `size` bytes.
    ///
    /// # Errors
    /// Returns [`Err`] if the `mmap()` system call fails and returns an `Error` representing
    /// the system error which occurred.
    ///
    pub fn new(size: usize) -> Result<Mapping> {
        Mapping::_new(size,libc::MAP_ANONYMOUS | libc::MAP_SHARED | libc::MAP_NORESERVE, -1)
    }

    /// Creates a new mapping of `size` bytes from the object referenced by file descriptor `fd`
    ///
    /// # Errors
    /// Returns [`Err`] if the `mmap()` system call fails and returns an `Error` representing
    /// the system error which occurred.
    ///
    pub fn new_from_fd(fd: RawFd, size: usize) -> Result<Mapping> {
        Mapping::_new(size, libc::MAP_SHARED, fd)
    }


    fn _new(size: usize, flags: libc::c_int, fd: RawFd) -> Result<Mapping> {
        let p = unsafe { mmap_allocate(size, flags, fd)? };
        Ok(Mapping {
            ptr: p,
            size
        })
    }

    /// Ensure that `offset` is not larger than this allocation.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] of kind `InvalidMappingOffset` if passed an
    /// illegal `offset`
    ///
    fn check_offset(&self, offset: usize) -> Result<()> {
        if offset > self.size {
            Err(Error::from(ErrorKind::InvalidMappingOffset(offset)))
        } else {
            Ok(())
        }
    }

    /// Return the pointer address of this allocation.
    pub fn address(&self) -> u64 {
        self.ptr as u64
    }

    /// Read and return an integer value in native byte order from `offset` into the memory allocation
    ///
    /// # Errors
    /// Returns [`Err`] of kind `InvalidMappingOffset` if passed an
    /// illegal `offset`
    ///
    pub fn read_int<T: Serializable>(&self, offset: usize) -> Result<T> {
        self.check_offset(offset + mem::size_of::<T>())?;
        unsafe {
           Ok(ptr::read_volatile(&self.as_slice()[offset..] as *const _ as *const T))
        }
    }

    /// Write the integer `val` in native byte order at `offset` into the memory allocation
    ///
    /// # Errors
    /// Returns [`Err`] of kind `InvalidMappingOffset` if passed an
    /// illegal `offset`
    ///
    pub fn write_int<T: Serializable>(&self, offset: usize, val: T) -> Result<()> {
        self.check_offset(offset + mem::size_of::<T>())?;
        unsafe { ptr::write_volatile(&mut self.as_mut_slice()[offset..] as *mut _ as *mut T, val); }
        Ok(())
    }

    pub fn write_bytes(&self, offset: usize, bytes: &[u8]) -> Result<()> {
        self.check_offset(offset + bytes.len())?;
        unsafe {
            let mut slice: &mut [u8] = &mut self.as_mut_slice()[offset..];
            slice.write_all(bytes).map_err(|_| Error::from(ErrorKind::InvalidMappingOffset(offset)))
        }
    }

    pub fn read_bytes(&self, offset: usize, mut bytes: &mut [u8]) -> Result<()> {
        self.check_offset(offset + bytes.len())?;
        unsafe {
            let slice: &[u8] = &self.as_slice()[offset..];
            bytes.write(slice).unwrap();
            Ok(())
        }
    }

    pub fn slice(&self, offset: usize, size: usize) -> Result<&[u8]> {
        self.check_offset(offset + size)?;
        unsafe {
            let x = &self.as_slice()[offset..offset+size];
            Ok(x)
        }
    }

    pub fn mut_slice(&self, offset: usize, size: usize) -> Result<&mut [u8]> {
        self.check_offset(offset + size)?;
        unsafe {
            let x = &mut self.as_mut_slice()[offset..offset+size];
            Ok(x)
        }
    }

    #[allow(dead_code)]
    pub fn set_mergeable(&self) -> Result<()> {
        unsafe {
            if libc::madvise(self.ptr as *mut libc::c_void, self.size, libc::MADV_MERGEABLE) == -1 {
                return Err(Error::from_last_errno());
            }
        }
        Ok(())
    }

    unsafe fn as_slice(&self) -> &[u8] {
        slice::from_raw_parts(self.ptr, self.size)
    }
    unsafe fn as_mut_slice(&self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.ptr, self.size)
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.size);
        }
    }
}

unsafe fn mmap_allocate(size: usize, flags: libc::c_int, fd: libc::c_int) -> Result<*mut u8> {
    let p = libc::mmap(ptr::null_mut(),
                   size, libc::PROT_READ|libc::PROT_WRITE,
                    flags, fd, 0);

    if p.is_null() || p == libc::MAP_FAILED {
        return Err(Error::from_last_errno());
    }
    Ok(p as *mut u8)
}