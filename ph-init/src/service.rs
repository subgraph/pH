use std::process::{Command, Child, Stdio};
use std::os::unix::process::CommandExt;
use std::path::{PathBuf, Path};

use crate::{Result, Error};
use std::{io, thread, env};
use crate::sys::_setsid;
use std::io::{Read, BufReader, BufRead};
use std::thread::JoinHandle;

#[derive(PartialEq)]
enum StdioMode {
    InheritAll,
    PipeOutput,
}

const BASE_ENVIRONMENT: &[&str] = &[
    "LANG=en_US.UTF8",
    "LC_COLLATE=C",
    "XDG_RUNTIME_DIR=/run/user/1000",
];

const SHELL_ENVIRONMENT: &[&str] = &[
    "SHELL=/bin/bash",
    "TERM=xterm-256color",
    "GNOME_DESKTOP_SESSION_ID=this-is-deprecated",
    "NO_AT_BRIDGE=1",
    "DISPLAY=:0",
    "XDG_SESSION_TYPE=wayland",
    "GDK_BACKEND=wayland",
    "WAYLAND_DISPLAY=wayland-0",
    "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
];

pub struct Service {
    name: String,
    child: Child,
    logthreads: Vec<JoinHandle<()>>,
}

impl Service {

    fn new(name: &str, child: Child) -> Self {
        let name = name.to_string();
        let logthreads = Vec::new();
        let mut service = Service { name, child, logthreads };
        service.log_output();
        service
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    fn log_output(&mut self) {
        if let Some(c) = self.child.stdout.take() {
            self.add_logger(ServiceLogger::new(&self.name, c))
        }
        if let Some(c) = self.child.stderr.take() {
            self.add_logger(ServiceLogger::new(&self.name, c))
        }
    }
    fn add_logger(&mut self, logger: ServiceLogger) {
        self.logthreads.push(logger.start())
    }
}

struct ServiceLogger {
    name: String,
    reader: Box<dyn Read+Send>,
}

impl ServiceLogger {
    fn new<T: Read + Send + 'static>(name: &str, reader: T) -> Self {
        ServiceLogger {
            name: name.to_string(),
            reader: Box::new(reader)
        }
    }

    fn start(self) -> JoinHandle<()> {
        thread::spawn({
            let mut reader = BufReader::new(self.reader);
            let name = self.name;
            move || Self::log_output(&mut reader,&name)})
    }

    fn log_output(reader: &mut BufReader<Box<dyn Read+Send>>, name: &str) {
        for line in reader.lines() {
            match line {
                Ok(line) => info!("{}: {}", name, line),
                Err(err) => {
                    warn!("{}: Error reading log output: {}", name, err);
                    return;
                }
            }
        }
    }
}

pub struct ServiceLaunch {
    name: String,
    home: String,
    exec: PathBuf,
    args: Vec<String>,
    env: Vec<(String,String)>,
    uid: u32,
    gid: u32,
    stdio: StdioMode,
}

impl ServiceLaunch {
    pub fn new<P: AsRef<Path>>(name: &str, exec: P) -> Self {
        let name = name.to_string();
        let exec = exec.as_ref().to_path_buf();
        ServiceLaunch {
            name,
            home: "/".to_string(),
            exec,
            args: Vec::new(),
            env: Vec::new(),
            uid: 0,
            gid: 0,
            stdio: StdioMode::InheritAll,
        }
    }

    pub fn new_shell<S>(root: bool, home: &str, realm: Option<S>) -> Self
        where S: Into<String>
    {
        let shell = Self::new("shell", "/bin/bash")
            .root(root)
            .home(home)
            .env("HOME", home)
            .shell_environment()
            .arg("--login");

        match realm {
            Some(name) => shell.env("REALM_NAME", name),
            None => shell
        }
    }

    pub fn base_environment(self) -> Self {
        self.env_list(BASE_ENVIRONMENT)
    }

    pub fn shell_environment(self) -> Self {
        self.env_list(BASE_ENVIRONMENT)
            .env_list(SHELL_ENVIRONMENT)
    }

    pub fn pipe_output(mut self) -> Self {
        self.stdio = StdioMode::PipeOutput;
        self
    }

    pub fn arg<S>(mut self, arg: S) -> Self
        where S: Into<String>
    {
        self.args.push(arg.into());
        self
    }

    pub fn env<K,V>(mut self, name: K, val: V) -> Self
        where K: Into<String>, V: Into<String>,
    {
        self.env.push((name.into(), val.into()));
        self
    }

    pub fn env_list<S>(mut self, vars: &[S]) -> Self
        where S: AsRef<str>
    {
        vars.iter().for_each(|v| {
            let v = v.as_ref();
            if let Some(idx) = v.find('=') {
                let (name,val) = v.split_at(idx);
                self.env.push((name.into(), val[1..].into()));
            }
        });
        self
    }

    pub fn uidgid(mut self, uid: u32, gid: u32) -> Self {
        self.uid = uid;
        self.gid = gid;
        self
    }

    pub fn root(self, root: bool) -> Self {
        if root {
            self.uidgid(0,0)
        } else {
            self.uidgid(1000,1000)
        }
    }

    pub fn home(mut self, home: &str) -> Self {
        self.home = home.to_string();
        self
    }

    fn output_stdio(&self) -> Stdio {
        match self.stdio {
            StdioMode::InheritAll => Stdio::inherit(),
            StdioMode::PipeOutput => Stdio::piped(),
        }
    }

    pub fn launch(self) -> Result<Service> {
        let home = self.home.clone();
        self.launch_with_preexec(move || {
            env::set_current_dir(&home)?;
            _setsid()?;
            Ok(())
        })
    }

    pub fn launch_with_preexec<F>(self, f: F) -> Result<Service>
        where F: FnMut() -> io::Result<()> + Sync + Send + 'static
    {
        info!("Starting: {}", self.name);
        unsafe {
            let child = Command::new(&self.exec)
                .stdout(self.output_stdio())
                .stderr(self.output_stdio())
                .args(&self.args)
                .envs(self.env.clone())
                .uid(self.uid)
                .gid(self.gid)
                .pre_exec(f)
                .spawn()
                .map_err(|e| {
                    let exec = self.exec.display().to_string();
                    Error::LaunchFailed(exec, e)
                })?;
            Ok(Service::new(&self.name, child))
        }
    }
}
