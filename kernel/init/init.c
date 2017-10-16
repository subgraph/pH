#include <sys/mount.h>
#include <string.h>
#include <unistd.h>
#include <stdio.h>
#include <errno.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/reboot.h>
#include <sys/syscall.h>

char *splash[] = {
"",
"             ------------------------------||-------------------------------",
"                                          [##]",
"                                        /~~~~~~\\",
"                                       |~~\\  /~~|",
"                                ==][===|___||___|===][==",
"                                 [::]  (   ()   )  [::]",
"                                        ~/~~~~\\~",
"                                       O'      `o", "",NULL
};

static int run_shell()
{
	char *new_argv[] = { "/bin/bash", NULL };
	char *new_env[] = { "TERM=xterm-256color", "HOME=/home/user", NULL };
	char **p;
	for(p = splash; *p; p++) {
		printf("%s\n", *p);
	}

	return execve("/bin/bash", new_argv, new_env);
}

static void do_mkdir(const char* path) {
	if(mkdir(path, 0755) < 0) {
		printf("mkdir %s failed: %s\n", path, strerror(errno));
	}

}

static void mount_tmpfs(const char *path) {
	if(mount("tmpfs", path, "tmpfs", 0, "mode=755") < 0) {
		printf("mount tmpfs to %s failed: %s\n", path, strerror(errno));
	}
}

static void pivot_root(const char *new_root, const char *put_old) {
	if(syscall(SYS_pivot_root, new_root, put_old) < 0) {
		printf("pivot_root failed (%s : %s) : %s\n", new_root, put_old, strerror(errno));
	}
}
static void move_mount(const char *source, const char *target) {
	if(mount(source, target, "", MS_MOVE, NULL) < 0) {
		printf("move mount of %s to %s failed: %s\n", source, target, strerror(errno));
	}
}


static void setup_overlay(void) {
	mount_tmpfs("/tmp");
	do_mkdir("/tmp/ro");
	do_mkdir("/tmp/rw");
	mount_tmpfs("/tmp/rw");
	do_mkdir("/tmp/rw/upper");
	do_mkdir("/tmp/rw/work");
	do_mkdir("/tmp/overlay");
	pivot_root("/tmp", "/tmp/ro");

	/*
	 *   /ro real root mounted here
	 *   /rw tmpfs mounted here
	 *   /rw/upper empty directory
	 *   /rw/work empty directory
	 *   /overlay empty directory
	 *
	 */
	if(mount("overlay", "/overlay", "overlay", 0, "lowerdir=/ro,upperdir=/rw/upper,workdir=/rw/work") < 0) {
		printf("mount overlay failed: %s\n", strerror(errno));
	}
	do_mkdir("/overlay/ro");
	do_mkdir("/overlay/rw");
	do_mkdir("/overlay/old-root");
	move_mount("/ro", "/overlay/ro");
	move_mount("/rw", "/overlay/rw");

	pivot_root("/overlay", "/overlay/old-root");
	umount("/old-root");
	umount("/ro/tmp");
}

static void do_mounts(void)
{

	mount("sysfs", "/sys", "sysfs", 0, NULL);
	mount("proc", "/proc", "proc", 0, NULL);
	mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
	mkdir("/dev/pts", 0755);
	mount("devpts", "/dev/pts", "devpts", 0, NULL);
}

int main(int argc, char *argv[])
{
	pid_t child;
	int status;

	setup_overlay();
	do_mounts();

	sethostname("airwolf", 7);
	/* get session leader */
	setsid();

	/* set controlling terminal */
	ioctl(0, TIOCSCTTY, 1);

	child = fork();
	if (child < 0) {
		printf("Fatal: fork() failed with %d\n", child);
		return 0;
	} else if (child == 0) {
		run_shell();
	} else {
		pid_t corpse;

		do {
			corpse = waitpid(-1, &status, 0);
		} while (corpse != child);
	}

	reboot(RB_AUTOBOOT);

	printf("Init failed: %s\n", strerror(errno));

	return 0;
}
