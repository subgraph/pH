use crate::virtio::{VirtioDeviceOps, VirtQueue, VirtioBus};
use crate::memory::MemoryManager;
use crate::{vm, system};
use std::sync::{RwLock, Arc};
use std::{fmt, result, thread, io};
use crate::system::{EPoll,Event};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use crate::system::Tap;

const VIRTIO_ID_NET: u16 = 1;
const MAC_ADDR_LEN: usize = 6;

#[derive(Debug)]
pub enum Error {
    ChainWrite(io::Error),
    ChainRead(io::Error),
    ChainIoEvent(vm::Error),
    SetupPoll(system::Error),
    TapRead(io::Error),
    TapWrite(io::Error),
    PollWait(system::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match self {
            ChainWrite(err) => write!(f, "Error writing to virtqueue chain: {}", err),
            ChainRead(err) => write!(f, "Error reading from virtqueue chain: {}", err),
            ChainIoEvent(err) => write!(f, "Error reading from virtqueue ioevent: {}", err),
            SetupPoll(e) => write!(f, "Failed to set up Poll: {}", e),
            TapRead(e) => write!(f, "Error reading from tap device: {}", e),
            TapWrite(e) => write!(f, "Error writing to tap device: {}", e),
            PollWait(e) => write!(f, "Poll wait returned error: {}", e),
        }
    }
}
type Result<T> = result::Result<T, Error>;


const VIRTIO_NET_F_CSUM: u64 = 1;
const VIRTIO_NET_F_GUEST_CSUM: u64 = 1 << 1;
const VIRTIO_NET_F_GUEST_TSO4: u64 = 1 << 7;
const VIRTIO_NET_F_GUEST_UFO: u64 = 1 << 10;
const VIRTIO_NET_F_HOST_TSO4: u64 = 1 << 11;
const VIRTIO_NET_F_HOST_UFO: u64 = 1 << 14;

//const VIRTIO_NET_HDR_SIZE: i32 = 12;

pub struct VirtioNet {
    tap: Option<Tap>,
}

impl VirtioNet {
    fn new(tap: Tap) -> Self {
        VirtioNet{
            tap: Some(tap)
        }
    }

    pub fn create(vbus: &mut VirtioBus, tap: Tap) -> vm::Result<()> {
        tap.set_offload(TUN_F_CSUM | TUN_F_UFO | TUN_F_TSO4 | TUN_F_TSO6).unwrap();
        tap.set_vnet_hdr_size(12).unwrap();
        let dev = Arc::new(RwLock::new(VirtioNet::new(tap)));
        let feature_bits =
                VIRTIO_NET_F_CSUM |
                VIRTIO_NET_F_GUEST_CSUM |
                VIRTIO_NET_F_GUEST_TSO4 |
                VIRTIO_NET_F_GUEST_UFO |
                VIRTIO_NET_F_HOST_TSO4 |
                VIRTIO_NET_F_HOST_UFO;

        vbus.new_virtio_device(VIRTIO_ID_NET, dev)
            .set_queue_sizes(&[256, 256])
            .set_config_size(MAC_ADDR_LEN)
            .set_features(feature_bits)
            .register()
    }
}

pub const TUN_F_CSUM: u32 = 1;
pub const TUN_F_TSO4: u32 = 2;
pub const TUN_F_TSO6: u32 = 4;
pub const TUN_F_UFO:  u32= 16;

impl VirtioDeviceOps for VirtioNet {
    fn start(&mut self, _memory: &MemoryManager, mut queues: Vec<VirtQueue>) {
        let tx = queues.pop().unwrap();
        let rx = queues.pop().unwrap();
        let tap = self.tap.take().unwrap();
        let poll = match EPoll::new() {
            Ok(poll) => poll,
            Err(e) => {
                warn!("Cannot start VirtioNet because unable to create Epoll instance: {}", e);
                return;
            }
        };
        let mut dev = VirtioNetDevice::new(rx, tx, tap, poll);
        thread::spawn(move || {
            if let Err(err) = dev.run() {
                warn!("error running virtio net device: {}", err);
            }
        });
    }
}

const MAX_BUFFER_SIZE: usize = 65562;
const RX_VQ_TOKEN:u64 = 1;
const TX_VQ_TOKEN:u64 = 2;
const RX_TAP:u64 = 3;

struct VirtioNetDevice {
    tap: Tap,
    poll: EPoll,
    tap_event_enabled: bool,
    rx: VirtQueue,
    tx: VirtQueue,
    rx_bytes: usize,
    rx_frame: [u8; MAX_BUFFER_SIZE],
    tx_frame: [u8; MAX_BUFFER_SIZE],
}

impl VirtioNetDevice {
    fn new(rx: VirtQueue, tx: VirtQueue, tap: Tap, poll: EPoll) -> Self {
        VirtioNetDevice {
            rx,
            tx,
            tap,
            poll,
            tap_event_enabled: false,
            rx_bytes: 0,
            rx_frame: [0; MAX_BUFFER_SIZE],
            tx_frame: [0; MAX_BUFFER_SIZE],
        }
    }

    fn enable_tap_poll(&mut self) {
        if !self.tap_event_enabled {
            if let Err(e) = self.poll.add_read(self.tap.as_raw_fd(), RX_TAP) {
                warn!("virtio_net: error enabling tap poll event: {}", e);
            } else {
                self.tap_event_enabled = true;
            }
        }
    }

    fn disable_tap_events(&mut self) {
        if self.tap_event_enabled {
            if let Err(e) = self.poll.delete(self.tap.as_raw_fd()) {
                warn!("virtio_net: error disabling tap poll event: {}", e);
            } else {
                self.tap_event_enabled = false;
            }
        }
    }

    fn handle_tx_queue(&mut self) -> Result<()> {
        self.tx.ioevent()
            .read()
            .map_err(Error::ChainIoEvent)?;

        while let Some(mut chain) = self.tx.next_chain() {
            loop {
                let n = chain.read(&mut self.tx_frame)
                    .map_err(Error::ChainRead)?;
                if n == 0 {
                    break;
                }
                self.tap.write_all(&self.tx_frame[..n])
                    .map_err(Error::TapWrite)?;
            }

            chain.skip_readable();
            chain.flush_chain()
        }
        Ok(())
    }

    fn pending_rx(&self) -> bool {
        self.rx_bytes != 0
    }

    fn receive_frame(&mut self) -> Result<bool> {
        if let Some(mut chain) = self.rx.next_chain() {
            chain.write_all(&self.rx_frame[..self.rx_bytes])
                .map_err(Error::ChainWrite)?;
            self.rx_bytes = 0;
            // XXX defer interrupt
            chain.flush_chain();
            Ok(true)
        } else {
            self.disable_tap_events();
            Ok(false)
        }
    }

    fn tap_read(&mut self) -> Result<bool> {
        match self.tap.read(&mut self.rx_frame) {
            Ok(n) => {
                self.rx_bytes = n;
                Ok(true)
            },
            Err(e) => if let Some(libc::EAGAIN) = e.raw_os_error() {
                // handle deferred interrupts
                Ok(false)
            } else {
                Err(Error::TapRead(e))
            },
        }
    }

    fn handle_rx_tap(&mut self) -> Result<()> {
        if self.pending_rx() {
            if !self.receive_frame()? {
                return Ok(())
            }
        }

        while self.tap_read()? {
            if !self.receive_frame()? {
                break;
            }
        }
        Ok(())
    }

    fn handle_rx_queue(&mut self) -> Result<()> {
        self.rx.ioevent().read().unwrap();
        if self.pending_rx() {
            if self.receive_frame()? {
                self.enable_tap_poll();
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, ev: Event) -> Result<()> {
        match ev.id() {
            TX_VQ_TOKEN => self.handle_tx_queue(),
            RX_VQ_TOKEN => self.handle_rx_queue(),
            RX_TAP=> self.handle_rx_tap(),
            _ => Ok(()),
        }
    }

    fn run(&mut self) -> Result<()> {
        self.poll.add_read(self.rx.ioevent().as_raw_fd(), RX_VQ_TOKEN)
            .map_err(Error::SetupPoll)?;
        self.poll.add_read(self.tx.ioevent().as_raw_fd(), TX_VQ_TOKEN)
            .map_err(Error::SetupPoll)?;
        self.enable_tap_poll();

        loop {
            let events = self.poll.wait().map_err(Error::PollWait)?;

            for ev in events.iter() {
                if let Err(err) = self.handle_event(ev) {
                    warn!("virtio_net: error handling poll event: {}", err);
                }
            }
        }
    }
}