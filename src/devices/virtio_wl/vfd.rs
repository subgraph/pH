use std::collections::{HashMap, VecDeque};
use std::io::{Write, SeekFrom};
use std::os::unix::io::{AsRawFd,RawFd};
use std::path::PathBuf;
use std::time::Duration;

use crate::memory::{MemoryManager, DrmDescriptor};
use crate::system::{FileDesc, FileFlags,EPoll,MemoryFd};
use crate::virtio::{VirtQueue, Chain};

use crate::devices::virtio_wl::{
    consts::*, Error, Result, shm::VfdSharedMemory, pipe::VfdPipe, socket::VfdSocket, VfdObject
};

pub struct VfdManager {
    wayland_path: PathBuf,
    mm: MemoryManager,
    use_transition_flags: bool,
    vfd_map: HashMap<u32, Box<dyn VfdObject>>,
    next_vfd_id: u32,
    poll_ctx: EPoll,
    in_vq: VirtQueue,
    in_queue_pending: VecDeque<PendingInput>,
}

impl VfdManager {
    fn round_to_page_size(n: usize) -> usize {
        let mask = 4096 - 1;
        (n + mask) & !mask
    }

    pub fn new<P: Into<PathBuf>>(mm: MemoryManager, use_transition_flags: bool, in_vq: VirtQueue, wayland_path: P) -> Result<Self> {
        let poll_ctx = EPoll::new().map_err(Error::FailedPollContextCreate)?;
        Ok(VfdManager {
            wayland_path: wayland_path.into(),
            mm, use_transition_flags,
            vfd_map: HashMap::new(),
            next_vfd_id: NEXT_VFD_ID_BASE,
            poll_ctx,
            in_vq,
            in_queue_pending: VecDeque::new(),
        })
    }

    pub fn get_vfd(&self, vfd_id: u32) -> Option<&dyn VfdObject> {
        self.vfd_map.get(&vfd_id).map(|vfd| vfd.as_ref())
    }

    pub fn get_mut_vfd(&mut self, vfd_id: u32) -> Option<&mut dyn VfdObject> {
        self.vfd_map.get_mut(&vfd_id)
            .map(|v| v.as_mut() as &mut dyn VfdObject)
    }


    pub fn create_pipe(&mut self, vfd_id: u32, is_local_write: bool) -> Result<()> {
        let pipe = VfdPipe::create(vfd_id, is_local_write)?;
        // XXX unwrap
        self.poll_ctx.add_read(pipe.poll_fd().unwrap(), vfd_id as u64)
            .map_err(Error::FailedPollAdd)?;
        self.vfd_map.insert(vfd_id, Box::new(pipe));
        Ok(())
    }

    pub fn create_shm(&mut self, vfd_id: u32, size: u32) -> Result<(u64,u64)> {
        let shm = VfdSharedMemory::create(vfd_id, self.use_transition_flags, size, &self.mm)?;
        let (pfn,size) = shm.pfn_and_size().unwrap();
        self.vfd_map.insert(vfd_id, Box::new(shm));
        Ok((pfn,size))
    }

    pub fn create_dmabuf(&mut self, vfd_id: u32, width: u32, height: u32, format: u32) -> Result<(u64, u64, DrmDescriptor)> {
        let (vfd, desc) = VfdSharedMemory::create_dmabuf(vfd_id, self.use_transition_flags, width, height, format, &self.mm)?;
        let (pfn, size) = vfd.pfn_and_size().unwrap();
        self.vfd_map.insert(vfd_id, Box::new(vfd));
        Ok((pfn, size, desc))
    }

    pub fn create_socket(&mut self, vfd_id: u32) -> Result<u32> {
        let sock = VfdSocket::open(vfd_id, self.use_transition_flags,&self.wayland_path)?;
        self.poll_ctx.add_read(sock.poll_fd().unwrap(), vfd_id as u64)
            .map_err(Error::FailedPollAdd)?;
        let flags = sock.flags();
        self.vfd_map.insert(vfd_id, Box::new(sock));
        Ok(flags)

    }

    pub fn poll_fd(&self) -> RawFd {
        self.poll_ctx.as_raw_fd()
    }

    pub fn in_vq_poll_fd(&self) -> RawFd {
        self.in_vq.ioevent().as_raw_fd()
    }

    pub fn process_poll_events(&mut self) {
        let events = match self.poll_ctx.wait_timeout(Duration::from_secs(0)) {
            Ok(v) => v.to_owned(),
            Err(e) => {
                warn!("Failed wait on wayland vfd events: {}", e);
                return;
            }
        };
        for ev in events.iter() {
            if ev.is_readable() {
                if let Err(e) = self.recv_from_vfd(ev.id() as u32) {
                    warn!("Error on wayland vfd recv(0x{:08x}): {}", ev.id() as u32, e);
                }
            } else if ev.is_hangup() {
                self.process_hangup_event(ev.id() as u32);
            }

        }

        if let Err(e) = self.drain_pending() {
            warn!("Error sending pending input: {}", e);
        }
    }

    fn drain_pending(&mut self) -> Result<()> {
        if self.in_queue_pending.is_empty() {
        }
        while !self.in_queue_pending.is_empty() {
            let mut chain = match self.in_vq.next_chain() {
                Some(chain) => chain,
                None => return Ok(()),
            };
            self.send_next_input_message(&mut chain)?;
        }
        Ok(())
    }

    fn process_hangup_event(&mut self, vfd_id: u32) {
        if let Some(vfd) = self.vfd_map.get(&vfd_id) {
            if let Some(fd) = vfd.poll_fd() {
                if let Err(e) = self.poll_ctx.delete(fd) {
                    warn!("failed to remove hangup vfd from poll context: {}", e);
                }
            }
        }
        self.in_queue_pending.push_back(PendingInput::new_hup(vfd_id));
    }

    fn recv_from_vfd(&mut self, vfd_id: u32) -> Result<()> {
        let vfd = match self.vfd_map.get_mut(&vfd_id) {
            Some(vfd) => vfd,
            None => return Ok(())
        };
        let recv = match vfd.recv()? {
            Some(recv) => recv,
            None => {
                self.in_queue_pending.push_back(PendingInput::new_hup(vfd_id));
                return Ok(())
            }
        };

        if let Some(fds) = recv.fds {
            let mut vfd_ids = Vec::new();
            for fd in fds {
                let vfd = self.vfd_from_file(self.next_vfd_id, fd)?;
                let id = self.add_vfd_device(vfd)?;
                vfd_ids.push(id);
            }
            self.in_queue_pending.push_back(PendingInput::new(vfd_id, Some(recv.buf), Some(vfd_ids)));
        } else {
            self.in_queue_pending.push_back(PendingInput::new(vfd_id, Some(recv.buf), None));
        }
        Ok(())
    }

    fn add_vfd_device(&mut self, vfd: Box<dyn VfdObject>) -> Result<u32> {
        let id = self.next_vfd_id;
        if let Some(poll_fd) = vfd.poll_fd() {
            self.poll_ctx.add_read(poll_fd, id as u64)
                .map_err(Error::FailedPollAdd)?;
        }
        self.vfd_map.insert(id, vfd);
        self.next_vfd_id += 1;
        Ok(id)
    }

    pub fn in_vq_ready(&mut self) -> Result<()> {
        self.in_vq.ioevent().read().map_err(Error::IoEventError)?;
        self.drain_pending()
    }

    fn send_next_input_message(&mut self, chain: &mut Chain) -> Result<()> {
        let pop = match self.in_queue_pending.front_mut() {
            Some(msg) => msg.send_message(chain, &self.vfd_map)?,
            None => false,
        };
        if pop {
            self.in_queue_pending.pop_front();
        }
        Ok(())
    }

    fn vfd_from_file(&self, vfd_id: u32, fd: FileDesc) -> Result<Box<dyn VfdObject>> {
        match fd.seek(SeekFrom::End(0)) {
            Ok(size) => {
                let size = Self::round_to_page_size(size as usize) as u64;
                let (pfn,slot) = self.mm.register_device_memory(fd.as_raw_fd(), size as usize)
                    .map_err(Error::RegisterMemoryFailed)?;

                let memfd = MemoryFd::from_filedesc(fd).map_err(Error::ShmAllocFailed)?;
                return Ok(Box::new(VfdSharedMemory::new(vfd_id, self.use_transition_flags,self.mm.clone(), memfd, slot, pfn)));
            }
            _ => {
                let flags = match fd.flags() {
                    Ok(FileFlags::Read) => VIRTIO_WL_VFD_READ,
                    Ok(FileFlags::Write) => VIRTIO_WL_VFD_WRITE,
                    Ok(FileFlags::ReadWrite) =>VIRTIO_WL_VFD_READ | VIRTIO_WL_VFD_WRITE,
                    _ => 0,
                };
                return Ok(Box::new(VfdPipe::local_only(vfd_id, fd, flags)));
            }
        }
    }

    pub fn close_vfd(&mut self, vfd_id: u32) -> Result<()> {
        if let Some(mut vfd) = self.vfd_map.remove(&vfd_id) {
            vfd.close()?;
        }
        // XXX remove any matching fds from in_queue_pending
        Ok(())
    }
}

struct PendingInput {
    vfd_id: u32,
    buf: Option<Vec<u8>>,
    vfds: Option<Vec<u32>>,
    // next index to transmit from vfds vector
    vfd_current: usize,
}

impl PendingInput {
    fn new_hup(vfd_id: u32) -> Self {
        Self::new(vfd_id, None, None)
    }

    fn new(vfd_id: u32, buf: Option<Vec<u8>>, vfds: Option<Vec<u32>>) -> Self {
        PendingInput { vfd_id, buf, vfds, vfd_current: 0 }
    }

    fn is_hup(&self) -> bool {
        self.buf.is_none() && self.vfds.is_none()
    }

    fn next_vfd(&mut self) -> Option<u32> {
        if let Some(ref vfds) = self.vfds {
            if self.vfd_current < vfds.len() {
                let id = vfds[self.vfd_current];
                self.vfd_current += 1;
                return Some(id);
            }
        }
        None
    }

    fn send_message(&mut self, chain: &mut Chain, vfd_map: &HashMap<u32, Box<dyn VfdObject>>) -> Result<bool> {
        let pop = if self.is_hup() {
            self.send_hup_message(chain)?;
            true
        } else if let Some(id) = self.next_vfd() {
            if let Some(vfd) = vfd_map.get(&id) {
                self.send_vfd_new_message(chain, vfd.as_ref())?;
            } else {
                warn!("No VFD found for vfd_id = {}", id)
            }
            false

        } else {
            self.send_recv_message(chain)?;
            true
        };
        Ok(pop)
    }

    fn send_hup_message(&self, chain: &mut Chain) -> Result<bool> {
        chain.w32(VIRTIO_WL_CMD_VFD_HUP)?;
        chain.w32(0)?;
        chain.w32(self.vfd_id)?;
        chain.flush_chain();
        Ok(true)
    }

    fn send_vfd_new_message(&self, chain: &mut Chain, vfd: &dyn VfdObject) -> Result<()> {
        chain.w32(VIRTIO_WL_CMD_VFD_NEW)?;
        chain.w32(0)?;
        chain.w32(vfd.id())?;
        chain.w32(vfd.flags())?;
        let (pfn, size) = match vfd.pfn_and_size() {
            Some(vals) => vals,
            None => (0,0),
        };
        chain.w64(pfn)?;
        chain.w32(size as u32)?;
        Ok(())
    }

    fn send_recv_message(&self, chain: &mut Chain) -> Result<bool> {
        chain.w32(VIRTIO_WL_CMD_VFD_RECV)?;
        chain.w32(0)?;
        chain.w32(self.vfd_id)?;
        if let Some(vfds) = self.vfds.as_ref() {
            chain.w32(vfds.len() as u32)?;
            for vfd_id in vfds {
                chain.w32(*vfd_id)?;
            }
        } else {
            chain.w32(0)?;
        }
        if let Some(buf) = self.buf.as_ref() {
            chain.write_all(buf)?;
        }
        chain.flush_chain();
        Ok(true)
    }
}

