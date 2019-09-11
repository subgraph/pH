use std::os::unix::io::RawFd;
use std::{result, io, fmt};

use crate::{vm, system};
use crate::memory::Error as MemError;
use crate::system::FileDesc;

mod vfd;
mod shm;
mod pipe;
mod socket;
mod device;

mod consts {
    use std::mem;

    pub const VIRTIO_ID_WL: u16 = 30;
    pub const VIRTWL_SEND_MAX_ALLOCS: usize = 28;
    pub const VIRTIO_WL_CMD_VFD_NEW: u32 = 256;
    pub const VIRTIO_WL_CMD_VFD_CLOSE: u32 = 257;
    pub const VIRTIO_WL_CMD_VFD_SEND: u32 = 258;
    pub const VIRTIO_WL_CMD_VFD_RECV: u32 = 259;
    pub const VIRTIO_WL_CMD_VFD_NEW_CTX: u32 = 260;
    pub const VIRTIO_WL_CMD_VFD_NEW_PIPE: u32 = 261;
    pub const VIRTIO_WL_CMD_VFD_HUP: u32 = 262;
    pub const VIRTIO_WL_RESP_OK: u32 = 4096;
    pub const VIRTIO_WL_RESP_VFD_NEW: u32 = 4097;
    pub const VIRTIO_WL_RESP_ERR: u32 = 4352;
    pub const VIRTIO_WL_RESP_OUT_OF_MEMORY: u32 = 4353;
    pub const VIRTIO_WL_RESP_INVALID_ID: u32 = 4354;
    pub const VIRTIO_WL_RESP_INVALID_TYPE: u32 = 4355;
    pub const VIRTIO_WL_RESP_INVALID_FLAGS: u32 = 4356;
    pub const VIRTIO_WL_RESP_INVALID_CMD: u32 = 4357;

    pub const VIRTIO_WL_VFD_WRITE: u32 = 0x1;          // Intended to be written by guest
    pub const VIRTIO_WL_VFD_READ: u32 = 0x2;           // Intended to be read by guest

    pub const VIRTIO_WL_VFD_MAP: u32 = 0x2;
    pub const VIRTIO_WL_VFD_CONTROL: u32 = 0x4;
    pub const VIRTIO_WL_F_TRANS_FLAGS: u32 = 0x01;

    pub const NEXT_VFD_ID_BASE: u32 = 0x40000000;
    pub const VFD_ID_HOST_MASK: u32 = NEXT_VFD_ID_BASE;

    pub const VFD_RECV_HDR_SIZE: usize = 16;
    pub const IN_BUFFER_LEN: usize =
        0x1000 - VFD_RECV_HDR_SIZE - VIRTWL_SEND_MAX_ALLOCS * mem::size_of::<u32>();
}

pub use device::VirtioWayland;
pub type Result<T> = result::Result<T, Error>;

pub struct VfdRecv {
    buf: Vec<u8>,
    fds: Option<Vec<FileDesc>>,
}

impl VfdRecv {
    fn new(buf: Vec<u8>) -> Self {
        VfdRecv { buf, fds: None }
    }
    fn new_with_fds(buf: Vec<u8>, fds: Vec<FileDesc>) -> Self {
        VfdRecv { buf, fds: Some(fds) }
    }
}

pub trait VfdObject {
    fn id(&self) -> u32;
    fn send_fd(&self) -> Option<RawFd> { None }
    fn poll_fd(&self) -> Option<RawFd> { None }
    fn recv(&mut self) -> Result<Option<VfdRecv>> { Ok(None) }
    fn send(&mut self, _data: &[u8]) -> Result<()> { Err(Error::InvalidSendVfd) }
    fn send_with_fds(&mut self, _data: &[u8], _fds: &[RawFd]) -> Result<()> { Err(Error::InvalidSendVfd) }
    fn flags(&self) -> u32;
    fn pfn_and_size(&self) -> Option<(u64, u64)> { None }
    fn close(&mut self) -> Result<()>;
}


#[derive(Debug)]
pub enum Error {
    IoEventError(vm::Error),
    ChainIoError(io::Error),
    UnexpectedCommand(u32),
    ShmAllocFailed(system::Error),
    RegisterMemoryFailed(MemError),
    CreatePipesFailed(system::Error),
    SocketReceive(system::Error),
    SocketConnect(io::Error),
    PipeReceive(io::Error),
    SendVfd(io::Error),
    InvalidSendVfd,
    TooManySendVfds(usize),
    FailedPollContextCreate(system::Error),
    FailedPollAdd(system::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            IoEventError(e) => write!(f, "error reading from ioevent fd: {}", e),
            ChainIoError(e) => write!(f, "i/o error on virtio chain operation: {}", e),
            UnexpectedCommand(cmd) => write!(f, "unexpected virtio wayland command: {}", cmd),
            ShmAllocFailed(e) => write!(f, "failed to allocate shared memory: {}", e),
            RegisterMemoryFailed(e) => write!(f, "failed to register memory with hypervisor: {}", e),
            CreatePipesFailed(e) => write!(f, "failed to create pipes: {}", e),
            SocketReceive(e) => write!(f, "error reading from socket: {}", e),
            SocketConnect(e) => write!(f, "error connecting to socket: {}", e),
            PipeReceive(e) => write!(f, "error reading from pipe: {}", e),
            SendVfd(e) => write!(f, "error writing to vfd: {}", e),
            InvalidSendVfd => write!(f, "attempt to send to incorrect vfd type"),
            TooManySendVfds(n) => write!(f, "message has too many vfd ids: {}", n),
            FailedPollContextCreate(e) => write!(f, "failed creating poll context: {}", e),
            FailedPollAdd(e) => write!(f, "failed adding fd to poll context: {}", e),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::ChainIoError(e)
    }
}
