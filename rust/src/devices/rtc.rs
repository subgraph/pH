use std::sync::{Arc,RwLock};
use std::mem;
use libc;

use vm::io::{IoDispatcher,IoPortOps};

const RTC_SECONDS: u8 = 0x00;
const RTC_MINUTES: u8 = 0x02;
const RTC_HOURS: u8 = 0x04;
const RTC_DAY_OF_WEEK: u8 = 0x06;
const RTC_DAY_OF_MONTH: u8 = 0x07;
const RTC_MONTH: u8 = 0x08;
const RTC_YEAR: u8 = 0x09;
const RTC_CENTURY: u8 = 0x32;

const RTC_REG_C: u8 = 0x0C;
const RTC_REG_D: u8 = 0x0D;

pub struct Rtc {
    idx: u8,
    data: [u8; 128]
}

impl IoPortOps for Rtc {
    fn io_in(&mut self, port: u16, _size: usize) -> u32 {
        if port == 0x0071 {
            self.data_in() as u32
        } else {
            0
        }
    }

    fn io_out(&mut self, port: u16, _size: usize, val: u32) {
        if port == 0x0070 {
            self.index_out(val as u8);
        } else if port == 0x0071 {
            self.data_out(val as u8)
        }
    }
}

impl Rtc {
    pub fn register(io: Arc<IoDispatcher>) {
        let rtc = Arc::new(RwLock::new(Rtc::new()));
        io.register_ioports(0x0070, 2, rtc);
    }

    fn new() -> Rtc {
        Rtc {
            idx:0,
            data: [0; 128]
        }
    }

    fn index_out(&mut self, data: u8) {
        let _nmi_disable = data & 0x80;
        self.idx = data & 0x7f;
    }

    fn data_in(&mut self) -> u8 {
        let now = RtcTime::now();
        match self.idx {
            RTC_SECONDS => now.seconds,
            RTC_MINUTES => now.minutes,
            RTC_HOURS => now.hours,
            RTC_DAY_OF_WEEK => now.wday,
            RTC_DAY_OF_MONTH => now.mday,
            RTC_MONTH => now.month,
            RTC_YEAR => now.year,
            RTC_CENTURY => now.century,
            _ => { self.data[self.idx as usize]},
        }
    }

    fn data_out(&mut self, data: u8) {
        if self.idx == RTC_REG_C || self.idx == RTC_REG_D {
            return;
        }
        self.data[self.idx as usize] = data;
    }
}

struct RtcTime {
    seconds: u8,
    minutes: u8,
    hours: u8,
    wday: u8,
    mday: u8,
    month: u8,
    year: u8,
    century: u8,
}

impl RtcTime {
    fn now() -> RtcTime {
        fn bcd(val: i32) -> u8 {
            (((val/10) << 4) + (val % 10)) as u8
        }
        unsafe {
            let mut tm: libc::tm = mem::zeroed();
            let mut time: libc::time_t = 0;
            libc::time(&mut time as *mut _);
            libc::gmtime_r(&time, &mut tm as *mut _);
            RtcTime {
                seconds: bcd(tm.tm_sec),
                minutes: bcd(tm.tm_min),
                hours: bcd(tm.tm_hour),
                wday: bcd(tm.tm_wday + 1),
                mday: bcd(tm.tm_mday),
                month: bcd(tm.tm_mon + 1),
                year: bcd(tm.tm_year % 100),
                century: bcd(tm.tm_year / 100),
            }
        }
    }
}
