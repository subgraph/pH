use std::sync::{Arc, RwLock};
use std::io::{self, Write};

use vm::io::{IoPortOps,IoDispatcher};
use kvm::Kvm;

const UART_TX: u16 = 0;
const UART_RX: u16 = 0;

const UART_IER: u16 = 1;
const UART_IER_RDI: u8 = 0x01;
const UART_IER_THRI: u8 = 0x02;

const UART_IIR: u16 = 2;
const UART_IIR_NO_INT: u8 = 0x01;
const UART_IIR_THRI: u8 = 0x02;
const UART_IIR_RDI: u8 = 0x04;
const UART_IIR_TYPE_BITS: u8 = 0xc0;

const UART_FCR: u16 = 2;
const UART_FCR_CLEAR_RCVR: u8 = 0x02;
const UART_FCR_CLEAR_XMIT: u8 = 0x04;

const UART_LCR: u16 = 3;
const UART_LCR_DLAB: u8 = 0x80;

const UART_MCR: u16 = 4;
const UART_MCR_LOOP: u8 = 0x10;
const UART_MCR_OUT2: u8 = 0x08;

const UART_LSR: u16 = 5;
const UART_LSR_TEMT: u8 = 0x40;
const UART_LSR_THRE: u8 = 0x20;
const UART_LSR_BI: u8 = 0x10;
const UART_LSR_DR: u8 = 0x01;

const UART_MSR: u16 = 6;
const UART_MSR_DCD: u8 = 0x80;
const UART_MSR_DSR: u8 = 0x20;
const UART_MSR_CTS: u8 = 0x10;

const UART_SCR: u16 = 7;

const FIFO_LEN: usize = 64;



trait Bits {
    fn set(&mut self, flag: Self);
    fn clear(&mut self, flag: Self);
    fn is_set(&self, flag: Self) -> bool;
}

impl Bits for u8 {
    fn set(&mut self, flag: u8) {
        *self |= flag;
    }

    fn clear(&mut self, flag: u8) {
        *self &= !flag;
    }

    fn is_set(&self, flag: u8) -> bool {
        *self & flag == flag
    }
}

pub struct SerialDevice {
    iobase: u16,
    kvm: Kvm,
    irq: u8,
    irq_state: u8,
    txcnt: usize,
    rxcnt: usize,
    rxdone: usize,
    txbuf: [u8; FIFO_LEN],
    rxbuf: [u8; FIFO_LEN],
    dll: u8,
    dlm: u8,
    iir: u8,
    ier: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    lsr: u8,
    msr: u8,
    scr: u8,
}

impl IoPortOps for SerialDevice {
    fn io_in(&mut self, port: u16, _size: usize) -> u32 {
        let off = port - self.iobase;
        self.serial_in(off) as u32
    }

    fn io_out(&mut self, port: u16, _size: usize, val: u32) {
        let off = port - self.iobase;
        self.serial_out(off, val as u8);
    }
}

impl SerialDevice {
    fn flush_tx(&mut self) {
        self.lsr.set(UART_LSR_TEMT | UART_LSR_THRE);
        if self.txcnt > 0 {
            io::stdout().write(&self.txbuf[..self.txcnt]).unwrap();
            self.txcnt = 0;
        }
    }

    fn update_irq(&mut self) {
        let mut iir = 0u8;
        if self.lcr.is_set(UART_FCR_CLEAR_RCVR) {
            self.lcr.clear(UART_FCR_CLEAR_RCVR);
            self.rxcnt = 0;
            self.rxdone = 0;
            self.lsr.clear(UART_LSR_DR);
        }

        if self.lcr.is_set(UART_FCR_CLEAR_XMIT) {
            self.lcr.clear(UART_FCR_CLEAR_XMIT);
            self.txcnt = 0;
            self.lsr.set(UART_LSR_TEMT|UART_LSR_THRE);
        }

        if self.ier.is_set(UART_IER_RDI) && self.lsr.is_set(UART_LSR_DR) {
            iir |= UART_IIR_RDI;
        }

        if self.ier.is_set(UART_IER_THRI) && self.lsr.is_set(UART_LSR_TEMT) {
            iir |= UART_IIR_THRI;
        }

        if iir == 0 {
            self.iir = UART_IIR_NO_INT;
            if self.irq_state != 0 {
                self.kvm.irq_line(self.irq as u32, 0).unwrap();
            }
        } else {
            self.iir = iir;
            if self.irq_state == 0 {
                self.kvm.irq_line(self.irq as u32, 1).unwrap();
            }
        }
        self.irq_state = iir;

        if !self.ier.is_set(UART_IER_THRI) {
            self.flush_tx();
        }
    }

    fn tx(&mut self, data: u8) {
        if self.lcr.is_set(UART_LCR_DLAB) {
            self.dll = data;
            return;
        }

        if self.mcr.is_set(UART_MCR_LOOP) {
            if self.rxcnt < FIFO_LEN {
                self.rxbuf[self.rxcnt] = data;
                self.rxcnt += 1;
                self.lsr.set(UART_LSR_DR);
            }
            return;
        }

        if self.txcnt < FIFO_LEN {
            self.txbuf[self.txcnt] = data;
            self.txcnt += 1;
            self.lsr.clear(UART_LSR_TEMT);
            if self.txcnt == FIFO_LEN / 2 {
                self.lsr.clear(UART_LSR_THRE);
            }
            self.flush_tx();
        } else {
            self.lsr.clear(UART_LSR_TEMT | UART_LSR_THRE);
        }
    }

    fn serial_out(&mut self,  port: u16, data: u8) {
        match port {
            UART_TX => {
                self.tx(data);
            },
            UART_IER => {
                if self.lcr.is_set(UART_LCR_DLAB) {
                    self.ier = data & 0x0f;
                } else {
                    self.dlm = data;
                }
            },
            UART_FCR => {
                self.fcr = data;
            },
            UART_LCR => {
                self.lcr = data;
            },
            UART_MCR => {
                self.mcr = data;
            },
            UART_LSR => {},
            UART_MSR => {},
            UART_SCR => {
                self.scr = data;
            },
            _ => {}
        }
        self.update_irq();
    }

    fn serial_in(&mut self, port: u16) -> u8 {
        let mut data = 0u8;
        match port {
            UART_RX => {
                if self.lcr.is_set(UART_LCR_DLAB) {
                    data = self.dll;
                } else {
                    self.rx(&mut data);
                }
            },
            UART_IER => {
                if self.lcr.is_set(UART_LCR_DLAB) {
                    data = self.dlm;
                } else {
                    data = self.ier;
                }
            },
            UART_IIR => {
                data = self.iir & UART_IIR_TYPE_BITS;
            },
            UART_LCR => {
                data = self.lcr;
            },
            UART_MCR => {
                data = self.mcr;
            },
            UART_LSR => {
                data = self.lsr;
            },
            UART_MSR => {
                data = self.msr;
            },
            UART_SCR => {
                data = self.scr;
            },
            _ => {},
        }
        self.update_irq();
        data
    }


    fn rx(&mut self, data: &mut u8) {
        if self.rxdone == self.rxcnt {
            return;
        }

        if self.lsr.is_set(UART_LSR_BI) {
            self.lsr.clear(UART_LSR_BI);
            *data = 0;
            return;
        }

        *data = self.rxbuf[self.rxdone];
        self.rxdone += 1;
        if self.rxdone == self.rxcnt {
            self.lsr.clear(UART_LSR_DR);
            self.rxdone = 0;
            self.rxcnt = 0;
        }
    }

    pub fn register(kvm: Kvm, io: Arc<IoDispatcher>, id: u8) {
        if let Some((base,irq)) = SerialDevice::base_irq_for_id(id) {
            let dev = SerialDevice::new(kvm, base, irq);
            io.register_ioports(base, 8, Arc::new(RwLock::new(dev)));
        }
    }

    fn base_irq_for_id(id: u8) -> Option<(u16, u8)> {
        match id {
            0 => Some((0x3f8, 4)),
            1 => Some((0x2f8, 3)),
            2 => Some((0x3e8, 4)),
            3 => Some((0x2e8, 3)),
            _ => None,
        }
    }

    fn new(kvm: Kvm, iobase: u16, irq: u8) -> SerialDevice {
        SerialDevice {
            iobase,
            kvm,
            irq,
            irq_state: 0,
            txcnt: 0,
            rxcnt: 0,
            rxdone:0,
            txbuf: [0; FIFO_LEN],
            rxbuf: [0; FIFO_LEN],
            dll: 0,
            dlm: 0,
            iir: UART_IIR_NO_INT,
            ier: 0,
            fcr: 0,
            lcr: 0,
            mcr: UART_MCR_OUT2,
            lsr: UART_LSR_TEMT | UART_LSR_THRE,
            msr: UART_MSR_DCD | UART_MSR_DSR | UART_MSR_CTS,
            scr: 0,
        }
    }
}
