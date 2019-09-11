use std::sync::{Arc,RwLock};
use std::io::{self,Write,Read};
use std::thread::spawn;
use termios::*;

use crate::virtio::{VirtioDeviceOps,VirtioBus, VirtQueue};
use crate::memory::MemoryManager;
use crate::vm::Result;

const VIRTIO_ID_CONSOLE: u16 = 3;

const VIRTIO_CONSOLE_F_SIZE: u64 = 0x1;
const VIRTIO_CONSOLE_F_MULTIPORT: u64 = 0x2;

const VIRTIO_CONSOLE_DEVICE_READY: u16  = 0;
const VIRTIO_CONSOLE_DEVICE_ADD: u16    = 1;
const _VIRTIO_CONSOLE_DEVICE_REMOVE: u16 = 2;
const VIRTIO_CONSOLE_PORT_READY: u16    = 3;
const VIRTIO_CONSOLE_CONSOLE_PORT: u16  = 4;
const VIRTIO_CONSOLE_RESIZE: u16        = 5;
const VIRTIO_CONSOLE_PORT_OPEN: u16     = 6;
const _VIRTIO_CONSOLE_PORT_NAME: u16     = 7;

pub struct VirtioSerial {
    feature_bits: u64,
}

impl VirtioSerial {
    fn new() -> VirtioSerial {
        VirtioSerial{feature_bits:0}
    }

    pub fn create(vbus: &mut VirtioBus) -> Result<()> {
        let dev = Arc::new(RwLock::new(VirtioSerial::new()));
        vbus.new_virtio_device(VIRTIO_ID_CONSOLE, dev)
            .set_num_queues(4)
            .set_device_class(0x0700)
            .set_config_size(12)
            .set_features(VIRTIO_CONSOLE_F_MULTIPORT|VIRTIO_CONSOLE_F_SIZE)
            .register()
    }

    fn start_console(&self, _memory: &MemoryManager, q: VirtQueue) {
        spawn(move || {
            loop {
                q.wait_ready().unwrap();
                for mut chain in q.iter() {
                    io::copy(&mut chain, &mut io::stdout()).unwrap();
                    io::stdout().flush().unwrap();
                }
            }
        });
    }

    fn multiport(&self) -> bool {
        self.feature_bits & VIRTIO_CONSOLE_F_MULTIPORT != 0
    }
}

use crate::system::ioctl;

#[repr(C)]
#[derive(Default)]
struct WinSz {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

const TIOCGWINSZ: u64 = 0x5413;

impl VirtioDeviceOps for VirtioSerial {
    fn reset(&mut self) {
        println!("Reset called");
    }

    fn enable_features(&mut self, bits: u64) -> bool {
        self.feature_bits = bits;
        true
    }


    fn start(&mut self, memory: &MemoryManager, mut queues: Vec<VirtQueue>) {
        let mut term = Terminal::create(queues.remove(0));
        self.start_console(memory, queues.remove(0));

        spawn( move || {
            term.read_loop();
        });

        if self.multiport() {
            let mut control = Control::new(queues.remove(0), queues.remove(0));
            spawn(move || {
                control.run();

            });
        }
    }

    fn read_config(&mut self, offset: usize, _size: usize) -> u64 {
        if offset == 4 {
            return 1;
        }
        0
    }
}

struct Control {
    rx_vq: VirtQueue,
    tx_vq: VirtQueue,
}

use byteorder::{LittleEndian,ReadBytesExt,WriteBytesExt};
impl Control {
    fn new(rx: VirtQueue, tx: VirtQueue) -> Control {
        Control { rx_vq: rx, tx_vq: tx }
    }

    fn run(&mut self) {
        let mut rx = self.rx_vq.clone();
        self.tx_vq.on_each_chain(|mut chain| {
            let _id = chain.read_u32::<LittleEndian>().unwrap();
            let event = chain.read_u16::<LittleEndian>().unwrap();
            let _value = chain.read_u16::<LittleEndian>().unwrap();
            if event == VIRTIO_CONSOLE_DEVICE_READY {
                Control::send_msg(&mut rx,0, VIRTIO_CONSOLE_DEVICE_ADD, 1).unwrap();
            }
            if event == VIRTIO_CONSOLE_PORT_READY {
                Control::send_msg(&mut rx,0, VIRTIO_CONSOLE_CONSOLE_PORT, 1).unwrap();
                Control::send_msg(&mut rx,0, VIRTIO_CONSOLE_PORT_OPEN, 1).unwrap();
                Control::send_resize(&mut rx, 0).unwrap();
            }
            chain.flush_chain();
        });

    }

    fn send_msg(vq: &mut VirtQueue, id: u32, event: u16, val: u16) -> io::Result<()> {
        let mut chain = vq.wait_next_chain().unwrap();
        chain.write_u32::<LittleEndian>(id)?;
        chain.write_u16::<LittleEndian>(event)?;
        chain.write_u16::<LittleEndian>(val)?;
        chain.flush_chain();
        Ok(())
    }

    fn send_resize(vq: &mut VirtQueue, id: u32) -> io::Result<()> {
        let (cols, rows) = Control::stdin_terminal_size()?;
        let mut chain = vq.wait_next_chain().unwrap();
        chain.write_u32::<LittleEndian>(id)?;
        chain.write_u16::<LittleEndian>(VIRTIO_CONSOLE_RESIZE)?;
        chain.write_u16::<LittleEndian>(0)?;
        chain.write_u16::<LittleEndian>(rows)?;
        chain.write_u16::<LittleEndian>(cols)?;
        chain.flush_chain();
        Ok(())
    }

    fn stdin_terminal_size() -> io::Result<(u16, u16)> {
        let mut wsz = WinSz{..Default::default()};
        unsafe {
            if let Err(err) = ioctl::ioctl_with_mut_ref(0, TIOCGWINSZ, &mut wsz) {
                println!("Got error calling TIOCGWINSZ on stdin: {:?}", err);
                return Err(io::Error::last_os_error());
            }
        }
        Ok((wsz.ws_col, wsz.ws_row))
    }

}

struct Terminal {
    saved: Termios,
    vq: VirtQueue,
}

impl Terminal {
    fn create(vq: VirtQueue) -> Terminal {
        let termios = Termios::from_fd(0).unwrap();
        Terminal {
            saved: termios,
            vq,
        }
    }

    fn setup_term(&self) {
        let mut termios = self.saved.clone();
        termios.c_iflag &= !(ICRNL);
        termios.c_lflag &= !(ISIG|ICANON|ECHO);
        let _ = tcsetattr(0, TCSANOW, &termios);
    }

    fn read_loop(&mut self) {
        self.setup_term();
        let mut abort_cnt = 0;
        let mut buf = vec![0u8; 32];
        loop {
            let n = io::stdin().read(&mut buf).unwrap();

            if n > 0 {
                // XXX write_all
                let mut chain = self.vq.wait_next_chain().unwrap();
                chain.write_all(&mut buf[..n]).unwrap();
                chain.flush_chain();
                if n > 1 || buf[0] != 3 {
                    abort_cnt = 0;
                } else {
                    abort_cnt += 1;
                }
            } else {
                println!("n = {}", n);
            }

            if abort_cnt == 3 {
                let _ = tcsetattr(0, TCSANOW, &self.saved);
            }

        }

    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = tcsetattr(0, TCSANOW, &self.saved);
    }
}
