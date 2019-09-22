
use crate::{Error,Result};
use crate::cmdline::CmdLine;
use crate::sys::{sethostname, setsid, set_controlling_tty, mount_devtmpfs, mount_tmpfs, mkdir, umount, mount_sysfs, mount_procfs, mount_devpts, chown, chmod, create_directories, mount_overlay, move_mount, pivot_root, mount_9p, mount, waitpid, reboot, getpid};
use std::path::Path;
use std::{fs, process, io};
use crate::service::{Service, ServiceLaunch};
use std::collections::BTreeMap;

pub struct InitServer {
    hostname: String,
    cmdline: CmdLine,
    rootfs: RootFS,
    services: BTreeMap<u32, Service>,
}

impl InitServer {
    pub fn create(hostname: &str) -> Result<InitServer> {
        Self::check_pid1()?;
        sethostname(hostname)?;
        setsid()?;
        set_controlling_tty(0, true)?;

        let hostname = hostname.to_string();
        let cmdline = CmdLine::load()?;
        let rootfs = RootFS::load(&cmdline)?;
        let services = BTreeMap::new();

        Ok(InitServer {
            hostname,
            cmdline,
            rootfs,
            services,
        })
    }

    fn check_pid1() -> Result<()> {
        if getpid() == 1 {
            Ok(())
        } else {
            Err(Error::Pid1)
        }
    }

    pub fn setup_filesystem(&self) -> Result<()> {
        mount_devtmpfs()?;
        mount_tmpfs("/tmp")?;
        mkdir("/tmp/sysroot")?;
        if self.rootfs.read_only() {
            self.setup_readonly_root()?;
        } else {
            self.setup_writeable_root()?;
        }
        umount("/opt/ph/tmp")?;
        umount("/opt/ph/proc")?;
        umount("/opt/ph/dev")?;

        mount_sysfs()?;
        mount_procfs()?;
        mount_devtmpfs()?;
        mount_devpts()?;
        mount_tmpfs("/run")?;
        mkdir("/run/user")?;
        mkdir("/run/user/1000")?;
        chown("/run/user/1000", 1000,1000)?;
        if Path::new("/dev/wl0").exists() {
            chmod("/dev/wl0", 0o666)?;
        }
        self.mount_home_if_exists()?;
        Ok(())
    }

    fn setup_readonly_root(&self) -> Result<()> {
        create_directories(&[
            "/tmp/ro",
            "/tmp/rw",
            "/tmp/rw/upper",
            "/tmp/rw/work",
        ])?;
        mount_tmpfs("/tmp/rw")?;
        create_directories(&["/tmp/rw/upper", "/tmp/rw/work"])?;
        self.rootfs.mount("/tmp/ro")?;
        mount_overlay("/tmp/sysroot",
                      "lowerdir=/tmp/ro,upperdir=/tmp/rw/upper,workdir=/tmp/rw/work")?;
        create_directories(&[
            "/tmp/sysroot/ro",
            "/tmp/sysroot/rw"
        ])?;
        move_mount("/tmp/ro", "/tmp/sysroot/ro")?;
        move_mount("/tmp/rw", "/tmp/sysroot/rw")?;

        let toolsdir = Path::new("/tmp/sysroot/opt/ph");
        if !toolsdir.exists() {
            fs::create_dir_all(toolsdir)
                .map_err(|e| Error::MkDir(String::from("/tmp/sysroot/opt/ph"), e))?;
        }
        pivot_root("/tmp/sysroot", "/tmp/sysroot/opt/ph")?;
        fs::write("/etc/hosts", format!("127.0.0.1       {} localhost", self.hostname))
            .map_err(Error::WriteEtcHosts)?;
        Ok(())
    }

    fn setup_writeable_root(&self) -> Result<()> {
        self.rootfs.mount("/tmp/sysroot")?;

        let toolsdir = Path::new("/tmp/sysroot/opt/ph");
        if !toolsdir.exists() {
            fs::create_dir_all(toolsdir)
                .map_err(|e| Error::MkDir(String::from("/tmp/sysroot/opt/ph"), e))?;
        }
        pivot_root("/tmp/sysroot", "/tmp/sysroot/opt/ph")?;
        Ok(())
    }

    fn has_9p_home(&self) -> bool {
        // XXX
        // /sys/bus/virtio/drivers/9pnet_virtio/virtio*/mount_tag
        true
    }

    pub fn mount_home_if_exists(&self) -> Result<()> {
        if self.has_9p_home() {
            let homedir = Path::new("/home/user");
            if !homedir.exists() {
                mkdir(homedir)?;
            }
            mount_9p("home", "/home/user")?;
        }
        Ok(())
    }

    pub fn run_daemons(&mut self) -> Result<()> {
        let dbus = ServiceLaunch::new("dbus-daemon", "/usr/bin/dbus-daemon")
            .base_environment()
            .uidgid(1000,1000)
            .arg("--session")
            .arg("--nosyslog")
            .arg("--address=unix:path=/run/user/1000/bus")
            .launch()?;

        self.services.insert(dbus.pid(), dbus);

        let sommelier = ServiceLaunch::new("sommelier", "/opt/ph/usr/bin/sommelier")
            .base_environment()
            .uidgid(1000,1000)
            .arg("--master")
            .launch()?;

        self.services.insert(sommelier.pid(), sommelier);

        Ok(())
    }

    pub fn launch_console_shell(&mut self, splash: &'static str) -> Result<()> {
        let root = self.cmdline.has_var("phinit.rootshell");
        let realm = self.cmdline.lookup("phinit.realm");

        let shell = ServiceLaunch::new_shell(root, realm)
            .launch_with_preexec(move || {
                println!("{}", splash);
                Ok(())
            })?;

        self.services.insert(shell.pid(), shell);

        Ok(())
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            if let Some(child) = self.wait_for_child() {
                println!("Service exited: {}", child.name());
                if child.name() == "shell" {
                    reboot(libc::RB_AUTOBOOT)
                        .map_err(Error::RebootFailed)?;
                }
            } else {
                println!("Unknown process exited.");
            }
        }
    }

    fn handle_waitpid_err(err: io::Error) -> ! {
        if let Some(errno) = err.raw_os_error() {
            if errno == libc::ECHILD {
                if let Err(err) = reboot(libc::RB_AUTOBOOT) {
                    println!("reboot() failed: {:?}", err);
                    process::exit(-1);
                }
            }
        }
        println!("error on waitpid: {:?}", err);
        process::exit(-1);
    }

    fn wait_for_child(&mut self) -> Option<Service> {
        match waitpid(-1, 0) {
            Ok((pid,_status)) => self.services.remove(&(pid as u32)),
            Err(err) => Self::handle_waitpid_err(err)
        }
    }
}
struct RootFS {
    root: String,
    fstype: String,
    rootflags: Option<String>,
    readonly: bool,
}

impl RootFS {
    fn load(cmdline: &CmdLine) -> Result<Self> {
        let root = cmdline.lookup("phinit.root")
            .ok_or(Error::NoRootVar)?;
        let fstype = cmdline.lookup("phinit.rootfstype")
            .ok_or(Error::NoRootFsVar)?;
        let rootflags = cmdline.lookup("phinit.rootflags");
        let readonly = !cmdline.has_var("phinit.root_rw");

        Ok(RootFS {
            root, fstype, rootflags, readonly
        })
    }

    fn read_only(&self) -> bool {
        self.readonly
    }

    fn mount(&self, target: &str) -> Result<()> {
        let options = self.rootflags.as_ref().map(|s| s.as_str());
        let flags = libc::MS_RDONLY;

        mount(&self.root, target, &self.fstype, flags, options)
            .map_err(|e| Error::RootFsMount(self.root.clone(), e))
    }
}
