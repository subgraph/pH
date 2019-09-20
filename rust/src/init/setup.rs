use crate::cmdline::CmdLine;
use crate::error::{Result,Error};
use std::{io, fs, process, env};
use crate::sys::{mount_procfs, mount_tmpfs, mkdir, mount_devpts, create_directories, mount_overlay, move_mount, pivot_root, umount, mount_sysfs, mount_devtmpfs, mount, mount_9p, waitpid, reboot, sethostname, setsid, set_controlling_tty, getpid, chmod, chown};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::cell::RefCell;
use std::os::unix::process::CommandExt;

pub struct Setup {
    hostname: String,
    cmdline: CmdLine,
    rootfs: RootFS,
    splash: RefCell<Option<String>>,
}

impl Setup {
    pub fn create(hostname: &str) -> Result<Self> {
        let hostname = hostname.to_string();
        let cmdline = CmdLine::load()?;
        let rootfs = RootFS::load(&cmdline)?;
        let splash = RefCell::new(None);
        Ok(Setup{ hostname, cmdline, rootfs, splash })
    }

    pub fn check_pid1() -> Result<()> {
        if getpid() == 1 {
            Ok(())
        } else {
            Err(Error::Pid1)
        }
    }

    pub fn setup_rootfs(&self) -> Result<()> {
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

    pub fn has_9p_home(&self) -> bool {
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

    fn wait_for_child(&self) -> i32 {
        match waitpid(-1, 0) {
            Ok(pid) => pid,
            Err(err) => Self::handle_waitpid_err(err)
        }
    }

    pub fn set_splash(&self, splash: &str) {
        self.splash.borrow_mut().replace(splash.to_string());
    }

    fn run_shell(&self, as_root: bool) -> io::Result<Child> {

        let home = if as_root {
            "/"
        } else {
            "/home/user"
        };
        env::set_current_dir(home)?;

        unsafe {
            let mut cmd = Command::new("/bin/bash");
            cmd.env_clear()
                .env("XDG_RUNTIME_DIR", "/run/user/1000")
                .env("HOME", home)
                .env("SHELL", "/bin/bash")

                .env("TERM", "xterm-256color")
                .arg("--login")
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            if let Some(s) = self.splash.borrow().as_ref() {
                let splash = s.to_string();
                cmd.pre_exec(move || {
                    println!("{}", splash);
                    Ok(())
                });
            }

            if let Some(realm) = self.cmdline.lookup("phinit.realm") {
                cmd.env("REALM_NAME", realm);
            }

            if !as_root {
                cmd.uid(1000);
                cmd.gid(1000);
            }
            cmd.spawn()
        }
    }

    pub fn launch_shell(&self) -> Result<()> {
        let as_root = self.cmdline.has_var("phinit.rootshell");
        sethostname("airwolf")?;
        setsid()?;
        set_controlling_tty(0, true)?;

        let _child = self.run_shell(as_root)
            .map_err(Error::RunShell)?;
        loop {
            let _ = self.wait_for_child();
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
