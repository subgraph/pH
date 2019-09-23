use std::fs::{OpenOptions, File};
use std::os::raw::{c_ulong, c_int, c_uint};
use std::os::unix::io::{RawFd,AsRawFd};
use std::path::Path;
use std::sync::Arc;

use crate::system::{self, ioctl::ioctl_with_mut_ref, FileDesc };
use crate::memory::{Error,Result};

#[derive(Default,Debug)]
pub struct DrmPlaneDescriptor {
    pub stride: u32,
    pub offset: u32,
}

#[derive(Default,Debug)]
pub struct DrmDescriptor {
    pub planes: [DrmPlaneDescriptor; 3]
}

#[derive(Clone)]
pub struct DrmBufferAllocator {
    dev: Arc<DrmDevice>,
}

impl DrmBufferAllocator {

    pub fn open() -> Result<Self> {
        let dev = DrmDevice::open_render_node()?;
        Ok(DrmBufferAllocator{
            dev: Arc::new(dev)
        })
    }

    pub fn allocate(&self, width: u32, height: u32, format: u32) -> Result<(FileDesc, DrmDescriptor)> {
        const GBM_BO_USE_LINEAR: u32 = 16;

        let buffer = self.create_buffer(width, height, format, GBM_BO_USE_LINEAR)?;
        let fd = buffer.buffer_fd()?;
        Ok((fd, buffer.drm_descriptor()))
    }

    fn create_buffer(&self, width: u32, height: u32, format: u32, flags: u32) -> Result<DrmBuffer> {
        let bo = unsafe {
            gbm_bo_create(self.dev.gbm, width, height, format, flags)
        };
        if bo.is_null() {
            let e = system::Error::last_os_error();
            Err(Error::GbmCreateBuffer(e))
        } else {
            Ok(DrmBuffer::new(self.dev.clone(), bo))
        }
    }
}

struct DrmDevice {
    file: File,
    gbm: *mut GbmDevice,
}

impl DrmDevice {
    fn open_render_node() -> Result<Self> {
        let path = Path::new("/dev/dri/renderD128");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(Error::OpenRenderNode)?;
        Self::create(file)
    }

    fn create(file: File) -> Result<Self> {
        let gbm = unsafe { gbm_create_device(file.as_raw_fd()) };
        if gbm.is_null() {
            let e = system::Error::last_os_error();
            Err(Error::GbmCreateDevice(e))
        } else {
            Ok(DrmDevice{ file, gbm })
        }
    }
}

impl Drop for DrmDevice {
    fn drop(&mut self) {
        unsafe { gbm_device_destroy(self.gbm); }
    }
}

impl AsRawFd for DrmDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

pub struct DrmBuffer {
    dev: Arc<DrmDevice>,
    bo: *mut GbmBo,
}
unsafe impl Send for DrmDevice {}
unsafe impl Sync for DrmDevice {}

impl DrmBuffer {
    fn new(dev: Arc<DrmDevice>, bo: *mut GbmBo) -> Self {
        DrmBuffer { dev, bo }
    }

    fn drm_descriptor(&self) -> DrmDescriptor {
        let mut desc = DrmDescriptor::default();
        for i in 0..self.plane_count() {
            if self.plane_handle(i) == self.plane_handle(0) {
                desc.planes[i].stride = self.plane_stride(i);
                desc.planes[i].offset = self.plane_offset(i);
            }
        }
        desc
    }

    fn plane_count(&self) -> usize {
        unsafe { gbm_bo_get_plane_count(self.bo) }
    }

    fn plane_handle(&self, plane: usize) -> u32 {
        unsafe { gbm_bo_get_handle_for_plane(self.bo, plane).u32 }
    }

    fn plane_offset(&self, plane: usize) -> u32 {
        unsafe { gbm_bo_get_offset(self.bo, plane) }
    }

    fn plane_stride(&self, plane: usize) -> u32 {
        unsafe { gbm_bo_get_stride_for_plane(self.bo, plane) }
    }

    fn buffer_fd(&self) -> Result<FileDesc> {
        const DRM_CLOEXEC: u32 = libc::O_CLOEXEC as u32;
        const DRM_RDWR: u32    = libc::O_RDWR as u32;
        let mut prime = DrmPrimeHandle {
            handle: self.plane_handle(0),
            flags: DRM_CLOEXEC | DRM_RDWR,
            ..Default::default()
        };

        unsafe {
            ioctl_with_mut_ref(self.dev.as_raw_fd(), DRM_IOCTL_PRIME_HANDLE_TO_FD, &mut prime)
                .map_err(Error::PrimeHandleToFD)?;
        }
        Ok(FileDesc::new(prime.fd))
    }
}

impl Drop for DrmBuffer {
    fn drop(&mut self) {
        unsafe { gbm_bo_destroy(self.bo); }
    }
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Hash, PartialEq, Eq)]
struct DrmPrimeHandle {
    pub handle: c_uint,
    pub flags: c_uint,
    pub fd: c_int,
}

const DRM_IOCTL_BASE: c_uint = 0x64;
const DRM_IOCTL_PRIME_HANDLE_TO_FD: c_ulong = iorw!(DRM_IOCTL_BASE, 0x2d, ::std::mem::size_of::<DrmPrimeHandle>() as i32);

#[repr(C)]
struct GbmDevice([u8; 0]);

#[repr(C)]
struct GbmBo([u8; 0]);

#[repr(C)]
pub union GbmBoHandle {
    pub u32: u32,
    _align: u64,
}

#[link(name = "gbm")]
extern "C" {
    fn gbm_bo_create(gbm: *mut GbmDevice, width: u32, height: u32, format: u32, flags: u32) -> *mut GbmBo;
    fn gbm_create_device(fd: libc::c_int) -> *mut GbmDevice;
    fn gbm_device_destroy(gbm: *mut GbmDevice);
    fn gbm_bo_destroy(bo: *mut GbmBo);
    fn gbm_bo_get_plane_count(bo: *mut GbmBo) -> usize;
    fn gbm_bo_get_handle_for_plane(bo: *mut GbmBo, plane: usize) -> GbmBoHandle;
    fn gbm_bo_get_offset(bo: *mut GbmBo, plane: usize) -> u32;
    fn gbm_bo_get_stride_for_plane(bo: *mut GbmBo, plane: usize) -> u32;
}


