use std::io::{self,Write};
use std::path::Path;
use std::os::unix::{net::UnixStream, io::{AsRawFd, RawFd}};

use crate::system::{FileDesc,ScmSocket};
use crate::devices::virtio_wl::{consts:: *, Error, Result, VfdObject, VfdRecv};

pub struct VfdSocket {
    vfd_id: u32,
    flags: u32,
    socket: Option<UnixStream>,
}

impl VfdSocket {
    pub fn open<P: AsRef<Path>>(vfd_id: u32, transition_flags: bool, path: P) -> Result<Self> {
        let flags = if transition_flags {
            VIRTIO_WL_VFD_READ | VIRTIO_WL_VFD_WRITE
        } else {
            VIRTIO_WL_VFD_CONTROL
        };
        let socket = UnixStream::connect(path)
            .map_err(Error::SocketConnect)?;
        socket.set_nonblocking(true)
            .map_err(Error::SocketConnect)?;

        Ok(VfdSocket{
            vfd_id,
            flags,
            socket: Some(socket),
        })
    }
    fn socket_recv(socket: &mut UnixStream) -> Result<(Vec<u8>, Vec<FileDesc>)> {
        let mut buf = vec![0; IN_BUFFER_LEN];
        let mut fd_buf = [0; VIRTWL_SEND_MAX_ALLOCS];
        let (len, fd_len) = socket.recv_with_fds(&mut buf, &mut fd_buf)
            .map_err(Error::SocketReceive)?;
        buf.truncate(len);
        let files = fd_buf[..fd_len].iter()
            .map(|&fd| FileDesc::new(fd)).collect();
        Ok((buf, files))
    }
}
impl VfdObject for VfdSocket {
    fn id(&self) -> u32 {
        self.vfd_id
    }

    fn send_fd(&self) -> Option<RawFd> {
        self.socket.as_ref().map(|s| s.as_raw_fd())
    }

    fn poll_fd(&self) -> Option<RawFd> {
        self.socket.as_ref().map(|s| s.as_raw_fd())
    }

    fn recv(&mut self) -> Result<Option<VfdRecv>> {
        if let Some(mut sock) = self.socket.take() {
            let (buf,files) = Self::socket_recv(&mut sock)?;
            if !(buf.is_empty() && files.is_empty()) {
                self.socket.replace(sock);
                if files.is_empty() {
                    return Ok(Some(VfdRecv::new(buf)));
                } else {
                    return Ok(Some(VfdRecv::new_with_fds(buf, files)));
                }
            }
        }
        Ok(None)
    }

    fn send(&mut self, data: &[u8]) -> Result<()> {
        if let Some(s) = self.socket.as_mut() {
            s.write_all(data).map_err(Error::SendVfd)
        } else {
            Err(Error::InvalidSendVfd)
        }
    }

    fn send_with_fds(&mut self, data: &[u8], fds: &[RawFd]) -> Result<()> {
        if let Some(s) = self.socket.as_mut() {
            s.send_with_fds(data, fds)
                .map_err(|_| Error::SendVfd(io::Error::last_os_error()))?;
            Ok(())
        } else {
            Err(Error::InvalidSendVfd)
        }
    }

    fn flags(&self) -> u32 {
        if self.socket.is_some() {
            self.flags
        } else {
            0
        }
    }
    fn close(&mut self) -> Result<()> {
        self.socket = None;
        Ok(())
    }
}


