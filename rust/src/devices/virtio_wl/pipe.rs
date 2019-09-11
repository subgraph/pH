use std::os::unix::io::{AsRawFd,RawFd};

use crate::system::{self,FileDesc};

use crate::devices::virtio_wl::{
    consts::{VIRTIO_WL_VFD_WRITE, VIRTIO_WL_VFD_READ, IN_BUFFER_LEN},
    Error, Result, VfdObject, VfdRecv,
};


pub struct VfdPipe {
    vfd_id: u32,
    flags: u32,
    local: Option<FileDesc>,
    remote: Option<FileDesc>,
}

impl VfdPipe {

    pub fn new(vfd_id: u32, read_pipe: FileDesc, write_pipe: FileDesc, local_write: bool) -> Self {
        if local_write {
            VfdPipe { vfd_id, local: Some(write_pipe), remote: Some(read_pipe), flags: VIRTIO_WL_VFD_WRITE }
        } else {
            VfdPipe { vfd_id, local: Some(read_pipe), remote: Some(write_pipe), flags: VIRTIO_WL_VFD_READ}
        }
    }

    pub fn local_only(vfd_id: u32, local_pipe: FileDesc, flags: u32) -> Self {
        VfdPipe { vfd_id, local: Some(local_pipe), remote: None, flags }
    }

    pub fn create(vfd_id: u32, local_write: bool) -> Result<Self> {
        let mut pipe_fds: [libc::c_int; 2] = [-1; 2];
        unsafe {
            if libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_CLOEXEC) < 0 {
                return Err(Error::CreatePipesFailed(system::Error::last_os_error()));
            }
            let read_pipe = FileDesc::new(pipe_fds[0]);
            let write_pipe = FileDesc::new(pipe_fds[1]);
            Ok(Self::new(vfd_id, read_pipe, write_pipe, local_write))
        }
    }
}

impl VfdObject for VfdPipe {
    fn id(&self) -> u32 {
        self.vfd_id
    }

    fn send_fd(&self) -> Option<RawFd> {
        self.remote.as_ref().map(|p| p.as_raw_fd())
    }

    fn poll_fd(&self) -> Option<RawFd> {
        self.local.as_ref().map(|p| p.as_raw_fd())
    }

    fn recv(&mut self) -> Result<Option<VfdRecv>> {
        if let Some(pipe) = self.local.take() {
            let mut buf = vec![0; IN_BUFFER_LEN];
            let len = pipe.read(&mut buf[..IN_BUFFER_LEN])
                .map_err(Error::PipeReceive)?;
            buf.truncate(len);
            if buf.len() > 0 {
                self.local.replace(pipe);
                return Ok(Some(VfdRecv::new(buf)));
            }
        }
        Ok(None)
    }

    fn send(&mut self, data: &[u8]) -> Result<()> {
        if let Some(pipe) = self.local.as_ref() {
            pipe.write_all(data).map_err(Error::SendVfd)
        } else {
            Err(Error::InvalidSendVfd)
        }
    }

    fn flags(&self) -> u32 {
        self.flags
    }

    fn close(&mut self) -> Result<()> {
        self.local = None;
        self.remote = None;
        Ok(())
    }
}
