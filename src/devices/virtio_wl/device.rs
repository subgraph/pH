use std::os::unix::io::{AsRawFd,RawFd};
use std::sync::{RwLock, Arc};
use std::thread;

use crate::{vm, system};
use crate::system::EPoll;
use crate::memory::MemoryManager;
use crate::virtio::{VirtQueue, EventFd, Chain, VirtioBus, VirtioDeviceOps};

use crate::devices::virtio_wl::{vfd::VfdManager, consts::*, Error, Result, VfdObject};

pub struct VirtioWayland {
    feature_bits: u64,
}

impl VirtioWayland {
    fn new() -> Self {
        VirtioWayland { feature_bits: 0 }
    }

    pub fn create(vbus: &mut VirtioBus) -> vm::Result<()> {
        let dev = Arc::new(RwLock::new(VirtioWayland::new()));
        vbus.new_virtio_device(VIRTIO_ID_WL, dev)
            .set_num_queues(2)
            .set_features(VIRTIO_WL_F_TRANS_FLAGS as u64)
            .register()
    }

    fn transition_flags(&self) -> bool {
        self.feature_bits & VIRTIO_WL_F_TRANS_FLAGS as u64 != 0
    }

    fn create_device(memory: MemoryManager, in_vq: VirtQueue, out_vq: VirtQueue, transition: bool) -> Result<WaylandDevice> {
        let kill_evt = EventFd::new().map_err(Error::IoEventError)?;
        let dev = WaylandDevice::new(memory, in_vq, out_vq, kill_evt, transition)?;
        Ok(dev)
    }
}

impl VirtioDeviceOps for VirtioWayland {
    fn enable_features(&mut self, bits: u64) -> bool {
        self.feature_bits = bits;
        true
    }

    fn start(&mut self, memory: &MemoryManager, mut queues: Vec<VirtQueue>) {
        thread::spawn({
            let memory = memory.clone();
            let transition = self.transition_flags();
            move || {
                let out_vq = queues.pop().unwrap();
                let in_vq = queues.pop().unwrap();
                let mut dev = match Self::create_device(memory.clone(), in_vq, out_vq,transition) {
                    Err(e) => {
                        warn!("Error creating virtio wayland device: {}", e);
                        return;
                    }
                    Ok(dev) => dev,
                };
                if let Err(e) = dev.run() {
                    warn!("Error running virtio-wl device: {}", e);
                };
            }
        });
    }
}

struct WaylandDevice {
    vfd_manager: VfdManager,
    out_vq: VirtQueue,
    kill_evt: EventFd,
}

impl WaylandDevice {
    const IN_VQ_TOKEN: u64 = 0;
    const OUT_VQ_TOKEN:u64 = 1;
    const KILL_TOKEN: u64 = 2;
    const VFDS_TOKEN: u64 = 3;

    fn new(mm: MemoryManager, in_vq: VirtQueue, out_vq: VirtQueue, kill_evt: EventFd, use_transition: bool) -> Result<Self> {
        let vfd_manager = VfdManager::new(mm, use_transition, in_vq, "/run/user/1000/wayland-0")?;
        Ok(WaylandDevice {
            vfd_manager,
            out_vq,
            kill_evt
        })
    }

    pub fn get_vfd(&self, vfd_id: u32) -> Option<&dyn VfdObject> {
        self.vfd_manager.get_vfd(vfd_id)
    }

    pub fn get_mut_vfd(&mut self, vfd_id: u32) -> Option<&mut dyn VfdObject> {
        self.vfd_manager.get_mut_vfd(vfd_id)
    }

    fn setup_poll(&mut self) -> system::Result<EPoll> {
        let poll = EPoll::new()?;
        poll.add_read(self.vfd_manager.in_vq_poll_fd(), Self::IN_VQ_TOKEN as u64)?;
        poll.add_read(self.out_vq.ioevent().as_raw_fd(), Self::OUT_VQ_TOKEN as u64)?;
        poll.add_read(self.kill_evt.as_raw_fd(), Self::KILL_TOKEN as u64)?;
        poll.add_read(self.vfd_manager.poll_fd(), Self::VFDS_TOKEN as u64)?;
        Ok(poll)
    }
    fn run(&mut self) -> Result<()> {
        let mut poll = self.setup_poll().map_err(Error::FailedPollContextCreate)?;

        'poll: loop {
            let events = match poll.wait() {
                Ok(v) => v,
                Err(e) => {
                    warn!("virtio_wl: error waiting for poll events: {}", e);
                    break;
                }
            };
            for ev in events.iter() {
                match ev.id() {
                    Self::IN_VQ_TOKEN => {
                        self.vfd_manager.in_vq_ready()?;
                    },
                    Self::OUT_VQ_TOKEN => {
                        self.out_vq.ioevent().read().map_err(Error::IoEventError)?;
                        if let Some(chain) = self.out_vq.next_chain() {
                            let mut handler = MessageHandler::new(self, chain);
                            match handler.run() {
                                Ok(()) => {
                                },
                                Err(err) => {
                                    warn!("virtio_wl: error handling request: {}", err);
                                    if !handler.responded {
                                        let _ = handler.send_err();
                                    }
                                },
                            }
                            handler.chain.flush_chain();
                        }
                    },
                    Self::KILL_TOKEN => break 'poll,
                    Self::VFDS_TOKEN => self.vfd_manager.process_poll_events(),
                    _ =>  warn!("virtio_wl: unexpected poll token value"),
                }
            };
        }
        Ok(())
    }
}

struct MessageHandler<'a> {
    device: &'a mut WaylandDevice,
    chain: Chain,
    responded: bool,
}

impl <'a> MessageHandler<'a> {

    fn new(device: &'a mut WaylandDevice, chain: Chain) -> Self {
        MessageHandler { device, chain, responded: false }
    }

    fn run(&mut self) -> Result<()> {
        let msg_type = self.chain.r32()?;
        // Flags are always zero
        let _flags = self.chain.r32()?;
        match msg_type {
            VIRTIO_WL_CMD_VFD_NEW => self.cmd_new_alloc(),
            VIRTIO_WL_CMD_VFD_CLOSE => self.cmd_close(),
            VIRTIO_WL_CMD_VFD_SEND => self.cmd_send(),
            VIRTIO_WL_CMD_VFD_NEW_CTX => self.cmd_new_ctx(),
            VIRTIO_WL_CMD_VFD_NEW_PIPE => self.cmd_new_pipe(),
            v => {
                self.send_invalid_command()?;
                Err(Error::UnexpectedCommand(v))
            },
        }
    }

    fn cmd_new_alloc(&mut self) -> Result<()> {
        let id = self.chain.r32()?;
        let flags = self.chain.r32()?;
        let _pfn = self.chain.r64()?;
        let size = self.chain.r32()?;

        match self.device.vfd_manager.create_shm(id, size) {
            Ok((pfn,size)) => self.resp_vfd_new(id, flags, pfn, size as u32),
            Err(Error::ShmAllocFailed(_)) => self.send_simple_resp(VIRTIO_WL_RESP_OUT_OF_MEMORY),
            Err(e) => Err(e),
        }
    }

    fn resp_vfd_new(&mut self, id: u32, flags: u32, pfn: u64, size: u32) -> Result<()> {
        self.chain.w32(VIRTIO_WL_RESP_VFD_NEW)?;
        self.chain.w32(0)?;
        self.chain.w32(id)?;
        self.chain.w32(flags)?;
        self.chain.w64(pfn)?;
        self.chain.w32(size as u32)?;
        self.responded = true;
        Ok(())
    }

    fn cmd_close(&mut self) -> Result<()> {
        let id = self.chain.r32()?;
        self.device.vfd_manager.close_vfd(id)?;
        self.send_ok()
    }

    fn cmd_send(&mut self) -> Result<()> {
        let id = self.chain.r32()?;

        let send_fds = self.read_vfd_ids()?;
        let data = self.chain.current_read_slice();

        let vfd = match self.device.get_mut_vfd(id) {
            Some(vfd) => vfd,
            None => return self.send_invalid_id(),
        };

        if let Some(fds) = send_fds.as_ref() {
            vfd.send_with_fds(data, fds)?;
        } else {
            vfd.send(data)?;
        }
        self.send_ok()
    }

    fn read_vfd_ids(&mut self) -> Result<Option<Vec<RawFd>>> {
        let vfd_count = self.chain.r32()? as usize;
        if vfd_count > VIRTWL_SEND_MAX_ALLOCS {
            return Err(Error::TooManySendVfds(vfd_count))
        }
        if vfd_count == 0 {
            return Ok(None);
        }

        let mut raw_fds = Vec::with_capacity(vfd_count);
        for _ in 0..vfd_count {
            let vfd_id = self.chain.r32()?;
            if let Some(fd) = self.vfd_id_to_raw_fd(vfd_id)? {
                raw_fds.push(fd);
            }
        }
        Ok(Some(raw_fds))
    }

    fn vfd_id_to_raw_fd(&mut self, vfd_id: u32) -> Result<Option<RawFd>> {
        let vfd = match self.device.get_vfd(vfd_id) {
            Some(vfd) => vfd,
            None => {
                warn!("virtio_wl: Received unexpected vfd id 0x{:08x}", vfd_id);
                return Ok(None);
            }
        };

        if let Some(fd) = vfd.send_fd() {
            Ok(Some(fd))
        } else {
            self.send_invalid_type()?;
            Err(Error::InvalidSendVfd)
        }
    }

    fn cmd_new_ctx(&mut self) -> Result<()> {
        let id = self.chain.r32()?;
        if !Self::is_valid_id(id) {
            return self.send_invalid_id();
        }
        let flags = self.device.vfd_manager.create_socket(id)?;
        self.resp_vfd_new(id, flags, 0, 0)?;
        Ok(())
    }

    fn cmd_new_pipe(&mut self) -> Result<()> {
        let id = self.chain.r32()?;
        let flags = self.chain.r32()?;

        if !Self::is_valid_id(id) {
            return self.send_invalid_id();
        }
        if !Self::valid_new_pipe_flags(flags) {
            notify!("invalid flags: 0x{:08}", flags);
            return self.send_invalid_flags();
        }

        let is_write = Self::is_flag_set(flags, VIRTIO_WL_VFD_WRITE);

        self.device.vfd_manager.create_pipe(id, is_write)?;

        self.resp_vfd_new(id, 0, 0, 0)
    }

    fn valid_new_pipe_flags(flags: u32) -> bool {
        // only VFD_READ and VFD_WRITE may be set
        if flags & !(VIRTIO_WL_VFD_WRITE|VIRTIO_WL_VFD_READ) != 0 {
            return false;
        }
        let read = Self::is_flag_set(flags, VIRTIO_WL_VFD_READ);
        let write = Self::is_flag_set(flags, VIRTIO_WL_VFD_WRITE);
        // exactly one of them must be set
        !(read && write) && (read || write)
    }

    fn is_valid_id(id: u32) -> bool {
        id & VFD_ID_HOST_MASK == 0
    }

    fn is_flag_set(flags: u32, bit: u32) -> bool {
        flags & bit != 0
    }

    fn send_invalid_flags(&mut self) -> Result<()> {
        self.send_simple_resp(VIRTIO_WL_RESP_INVALID_FLAGS)
    }

    fn send_invalid_id(&mut self) -> Result<()> {
        self.send_simple_resp(VIRTIO_WL_RESP_INVALID_ID)
    }

    fn send_invalid_type(&mut self) -> Result<()> {
        self.send_simple_resp(VIRTIO_WL_RESP_INVALID_TYPE)
    }

    fn send_invalid_command(&mut self) -> Result<()> {
        self.send_simple_resp(VIRTIO_WL_RESP_INVALID_CMD)
    }

    fn send_ok(&mut self) -> Result<()> {
        self.send_simple_resp(VIRTIO_WL_RESP_OK)
    }

    fn send_err(&mut self) -> Result<()> {
        self.send_simple_resp(VIRTIO_WL_RESP_ERR)
    }

    fn send_simple_resp(&mut self, code: u32) -> Result<()> {
        self.chain.w32(code)?;
        self.responded = true;
        Ok(())
    }
}
