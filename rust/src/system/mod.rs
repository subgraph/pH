#[macro_use]pub mod ioctl;
mod epoll;
mod errno;
mod bitvec;
mod socket;
mod filedesc;
mod memfd;

pub use bitvec::BitVec;
pub use filedesc::{FileDesc, FileFlags};
pub use memfd::MemoryFd;
pub use epoll::EPoll;
pub use errno::{Error,Result,errno_result};
pub use socket::ScmSocket;

use errno::cvt;
