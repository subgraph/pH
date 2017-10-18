
LINUX_VERSION = 4.9.56
MINIPLI_TAG = 20171013093040


MINIPLI_VERSION = v$(LINUX_VERSION)-unofficial_grsec
MINIPLI_PATH    = linux-unofficial_grsec/releases/download/$(MINIPLI_VERSION)
GRSEC_DIFF      = $(MINIPLI_VERSION)-$(MINIPLI_TAG).diff
GRSEC_DOWNLOAD  = https://github.com/minipli/$(MINIPLI_PATH)/$(GRSEC_DIFF)

KERNEL_VERSION  = linux-$(LINUX_VERSION)
KERNEL_DOWNLOAD = https://cdn.kernel.org/pub/linux/kernel/v4.x/$(KERNEL_VERSION)


WGET    = wget
SHA256  = sha256sum
TAR     = tar
PATCH   = patch
LN	= ln
CP      = cp
MV	= mv
RM	= rm
RMDIR	= rmdir 
MKDIR   = mkdir
TOUCH 	= touch
GPG 	= gpg
TOUCH   = touch
UNXZ	= unxz

PWD	= $(shell pwd)

PATCH_FILES := $(shell find patches/ -name "00*.patch" | sort)
PATCH_SENTINEL = build/.patches_applied


GRSEC_DL_PATH = downloads/$(GRSEC_DIFF)
KERNEL_DL_PATH = downloads/$(KERNEL_VERSION).tar
KERNEL_BUILD_PATH = build/$(KERNEL_VERSION)
KERNEL_UNPACK_PATH = build/.unpack/$(KERNEL_VERSION)
KERNEL_BUILD_OUTPUT = $(KERNEL_BUILD_PATH)/vmlinux

PH_LINUX = ph_linux
PH_LINUX_VER = $(PH_LINUX)-$(LINUX_VERSION)

GPGTMP = verify-tmp/
GPGOPTS = --homedir $(GPGTMP) --trust-model direct --no-autostart
GPG_IMPORT = $(GPG) $(GPGOPTS) --import
GPG_VERIFY = $(GPG) $(GPGOPTS) --verify

SHA256_FILE = v$(KERNEL_VERSION).sha256

SHA256_SENTINEL = build/.sha256-verififed

$(shell mkdir -p downloads)
$(shell mkdir -p build)

.NOTPARALLEL:

.PHONY: all gen-sha256

all: $(KERNEL_BUILD_OUTPUT) $(PH_LINUX_VER)
	
$(GRSEC_DL_PATH):
	cd downloads; $(WGET) $(GRSEC_DOWNLOAD)
	cd downloads; $(WGET) $(GRSEC_DOWNLOAD).sig

$(KERNEL_DL_PATH):
	cd downloads; $(WGET) $(KERNEL_DOWNLOAD).tar.xz
	cd downloads; $(UNXZ) $(KERNEL_VERSION).tar.xz
	cd downloads; $(WGET) $(KERNEL_DOWNLOAD).tar.sign


$(KERNEL_BUILD_PATH): $(KERNEL_DL_PATH) $(GRSEC_DL_PATH) $(SHA256_SENTINEL)
	$(RM) -rf build/.unpack
	$(MKDIR) -p build/.unpack
	$(TAR) -C build/.unpack -xvf downloads/$(KERNEL_VERSION).tar
	$(PATCH) -p1 -d $(KERNEL_UNPACK_PATH) < $(GRSEC_DL_PATH)
	$(CP) config $(KERNEL_UNPACK_PATH)/.config
	$(MV) $(KERNEL_UNPACK_PATH) build/
	$(RM) -rf build/.unpack
	
$(PATCH_SENTINEL): | $(KERNEL_BUILD_PATH)
	for p in $(PATCH_FILES); do $(PATCH) -p1 -d build/$(KERNEL_VERSION) < "$$p"; done
	$(TOUCH) $@


$(PH_LINUX_VER): $(KERNEL_BUILD_OUTPUT)
	$(RM) -f $(PH_LINUX)
	$(CP) $(KERNEL_BUILD_OUTPUT) $(PH_LINUX_VER)
	$(LN) -s $(PWD)/$(PH_LINUX_VER) $(PH_LINUX)


$(KERNEL_BUILD_OUTPUT): $(PATCH_SENTINEL)
	$(MAKE) -C build/$(KERNEL_VERSION)

clean:
	$(RM) -rf $(KERNEL_BUILD_PATH) $(SHA256_SENTINEL) $(PATCH_SENTINEL) $(PH_LINUX_VER)

$(SHA256_SENTINEL): $(KERNEL_DL_PATH) $(GRSEC_DL_PATH)
	$(SHA256) -c v$(LINUX_VERSION).sha256
	$(TOUCH) $@


gen-sha256: $(SHA256_FILE)

$(GRSEC_DL_PATH).verify: $(GRSEC_DL_PATH)
	rm -rf $(GPGTMP)
	$(MKDIR) -m 0700 $(GPGTMP)
	$(GPG_IMPORT) < keys/minipli.key
	$(GPG_VERIFY) downloads/$(GRSEC_DIFF).sig $(GRSEC_DL_PATH) 2> .out
	mv .out $(GRSEC_DL_PATH).verify
	rm -rf $(GPGTMP)

$(KERNEL_DL_PATH).verify: $(KERNEL_DL_PATH)
	rm -rf $(GPGTMP)
	$(MKDIR) -m 0700 $(GPGTMP)
	$(GPG_IMPORT) < keys/torvalds.key
	$(GPG_IMPORT) < keys/gregkh.key
	$(GPG_VERIFY) downloads/$(KERNEL_VERSION).tar.sign $(KERNEL_DL_PATH) 2> .out
	mv .out $(KERNEL_DL_PATH).verify
	rm -rf $(GPGTMP)


$(SHA256_FILE): $(KERNEL_DL_PATH).verify $(GRSEC_DL_PATH).verify
	$(SHA256) $(KERNEL_DL_PATH) $(GRSEC_DL_PATH) > v$(LINUX_VERSION).sha256

