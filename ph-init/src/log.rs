
use std::sync::Mutex;
use std::io::{self,Write};
use std::fs::{File, OpenOptions};
use std::path::Path;

lazy_static! {
    static ref LOGGER: Mutex<Logger> = Mutex::new(Logger::new());
}

#[macro_export]
macro_rules! debug {
    ($e:expr) => { $crate::Logger::log($crate::LogLevel::Debug, String::from($e)) };
    ($fmt:expr, $($arg:tt)+) => { $crate::Logger::log($crate::LogLevel::Debug, format!($fmt, $($arg)+)) };
}

#[macro_export]
macro_rules! verbose {
    ($e:expr) => { $crate::Logger::log($crate::LogLevel::Verbose, String::from($e)) };
    ($fmt:expr, $($arg:tt)+) => { $crate::Logger::log($crate::LogLevel::Verbose, format!($fmt, $($arg)+)) };
}

#[macro_export]
macro_rules! info {
    ($e:expr) => { $crate::Logger::log($crate::LogLevel::Info, String::from($e)) };
    ($fmt:expr, $($arg:tt)+) => { $crate::Logger::log($crate::LogLevel::Info, format!($fmt, $($arg)+)) };
}

#[macro_export]
macro_rules! notify {
    ($e:expr) => { $crate::Logger::log($crate::LogLevel::Notice, String::from($e)) };
    ($fmt:expr, $($arg:tt)+) => { $crate::Logger::log($crate::LogLevel::Notice, format!($fmt, $($arg)+)) };
}

#[macro_export]
macro_rules! warn {
    ($e:expr) => { $crate::Logger::log($crate::LogLevel::Warn, String::from($e)) };
    ($fmt:expr, $($arg:tt)+) => { $crate::Logger::log($crate::LogLevel::Warn, format!($fmt, $($arg)+)) };
}

#[derive(PartialOrd,PartialEq,Copy,Clone)]
pub enum LogLevel {
    Warn,
    Notice,
    Info,
    Verbose,
    Debug,
}

pub trait LogOutput: Send {
    fn log_output(&mut self, level: LogLevel, line: &str) -> io::Result<()>;
}

pub struct Logger {
    level: LogLevel,
    output: Box<dyn LogOutput>,
}

impl Logger {
    pub fn set_log_level(level: LogLevel) {
        let mut logger = LOGGER.lock().unwrap();
        logger.level = level;
    }

    pub fn set_log_output(output: Box<dyn LogOutput>) {
        let mut logger = LOGGER.lock().unwrap();
        logger.output = output;
    }

    pub fn set_file_output<P: AsRef<Path>>(path: P) -> io::Result<()> {
        let output = FileLogOutput::open(path.as_ref())?;
        Self::set_log_output(Box::new(output));
        Ok(())
    }

    pub fn log(level: LogLevel, message: impl AsRef<str>) {
        let mut logger = LOGGER.lock().unwrap();
        logger.log_message(level, message.as_ref());
    }

    fn new() -> Self {
        Self { level: LogLevel::Notice, output: Box::new(DefaultLogOutput::new()) }
    }

    fn log_message(&mut self, level: LogLevel, message: &str) {
        if self.level >= level {
            if let Err(err) = self.output.log_output(level, message) {
                eprintln!("Error writing logline: {}", err);
            }
        }
    }

    pub fn format_logline(level: LogLevel, line: &str) -> String {
        let prefix = match level {
            LogLevel::Debug   => "[.]",
            LogLevel::Verbose => "[-]",
            LogLevel::Info    => "[+]",
            LogLevel::Notice  => "[*]",
            LogLevel::Warn    => "[Warning]",
        };
        format!("{} {}\n", prefix, line)
    }
}

#[derive(Clone,Default)]
pub struct DefaultLogOutput;

impl DefaultLogOutput {
    pub fn new() -> Self { DefaultLogOutput::default() }
}

impl LogOutput for DefaultLogOutput {
    fn log_output(&mut self, level: LogLevel, line: &str) -> io::Result<()> {
        let line = Logger::format_logline(level, line);

        let stdout = io::stdout();
        let mut lock = stdout.lock();
        lock.write_all(line.as_bytes())?;
        lock.flush()?;
        Ok(())
    }
}

struct FileLogOutput(Mutex<File>);
impl FileLogOutput {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(FileLogOutput(Mutex::new(file)))
    }
}

impl LogOutput for FileLogOutput {
    fn log_output(&mut self, level: LogLevel, line: &str) -> io::Result<()> {
        let line = Logger::format_logline(level, line);
        let mut lock = self.0.lock().unwrap();
        lock.write_all(line.as_bytes())
    }
}
