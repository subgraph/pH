use std::fmt::{self, Display};
use std::io;
use std::result;

use libc::__errno_location;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Error(i32);
pub type Result<T> = result::Result<T, Error>;

impl Error {
    pub fn from_raw_os_error(e: i32) -> Error {
        Error(e)
    }

    pub fn last_os_error() -> Error {
        Error(unsafe { *__errno_location() })
    }

    pub fn errno(self) -> i32 {
        self.0
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::from_raw_os_error(e.raw_os_error().unwrap_or_default())
    }
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        io::Error::from_raw_os_error(self.0).fmt(f)
    }
}

pub fn errno_result<T>() -> Result<T> {
    Err(Error::last_os_error())
}

pub fn cvt<T: IsMinusOne>(t: T) -> io::Result<T> {
    if t.is_minus_one() {
        Err(io::Error::last_os_error())
    } else {
        Ok(t)
    }
}

pub trait IsMinusOne {
    fn is_minus_one(&self) -> bool;
}

impl IsMinusOne for i32 {
    fn is_minus_one(&self) -> bool {
        *self == -1
    }
}
impl IsMinusOne for i64 {
    fn is_minus_one(&self) -> bool {
        *self == -1
    }
}

impl IsMinusOne for isize {
    fn is_minus_one(&self) -> bool {
        *self == -1
    }
}
