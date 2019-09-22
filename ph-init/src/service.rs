use std::process::Command;
use std::ffi::OsStr;

const DEFAULT_ENVIRONMENT: &[&str] = &[
    "SHELL=/bin/bash",
    "TERM=xterm-256-color",
    "LANG=en_US.UTF8",
    "LC_COLLATE=C",
    "GNOME_DESKTOP_SESSION_ID=this-is-deprecated",
    "XDR_RUNTIME_DIR=/run/user/1000",
    "NO_AT_BRIDGE=1",
    "DISPLAY=:0",
    "XDG_SESSION_TYPE=wayland",
    "GDK_BACKEND=wayland",
    "WAYLAND_DISPLAY=wayland-0",
    "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
];

pub struct Launcher {
    command: Command,
    uidgid: Option((u32,u32)),
    environment: Vec<String>,

}

impl Launcher {

    pub fn new(cmd: &str) -> Self {
        let command = Command::new(cmd);
        let uidgid = None;
        let environment = Vec::new();

        Launcher { command, uidgid, environment }
    }

    pub fn new_shell(root: bool) -> Self {
        let mut launcher = Self::new("/bin/bash");

        if root {
            launcher.env("HOME", "/");
        } else {
            launcher.env("HOME", "/home/user");
            launcher.uidgid = Some((1000,1000));
        }
        launcher
    }

    pub fn env<K,V>(&mut self, name: K, val: V)
        where K: AsRef<OsStr>, V: AsRef<OsStr>
    {
        self.command.env(name, val);
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) {
        self.command.arg(arg);
    }

}