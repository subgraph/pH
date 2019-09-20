
LINUX_VERSION = 5.3

KERNEL_VERSION  = linux-$(LINUX_VERSION)
KERNEL_DOWNLOAD = https://cdn.kernel.org/pub/linux/kernel/v5.x/$(KERNEL_VERSION)

PWD	= $(shell pwd)
NPROC	= $(shell nproc)

PATCH_FILES := $(shell find patches/ -name "00*.patch" | sort)

KERNEL_DL_PATH = downloads/$(KERNEL_VERSION).tar
KERNEL_BUILD_PATH = $(KERNEL_VERSION)
KERNEL_BUILD_OUTPUT = $(KERNEL_BUILD_PATH)/vmlinux

PH_LINUX = ph_linux
PH_LINUX_VER = $(PH_LINUX)-$(LINUX_VERSION)

SHA256_FILE = v$(KERNEL_VERSION).sha256

$(shell mkdir -p downloads)
$(shell mkdir -p .work)

SHA256_SENTINEL = .work/.sha256-verififed

.PHONY: all gen-sha256

all: $(KERNEL_BUILD_OUTPUT) $(PH_LINUX_VER)
	
$(KERNEL_DL_PATH):
	cd downloads; wget $(KERNEL_DOWNLOAD).tar.xz
	cd downloads; unxz $(KERNEL_VERSION).tar.xz
	cd downloads; wget $(KERNEL_DOWNLOAD).tar.sign


$(KERNEL_BUILD_PATH): $(KERNEL_DL_PATH) $(SHA256_SENTINEL)
	rm -rf .work/$(KERNEL_VERSION)
	tar -C .work -xf downloads/$(KERNEL_VERSION).tar
	for p in $(PATCH_FILES); do patch -p1 -d .work/$(KERNEL_VERSION) < "$$p"; done
	cp config .work/$(KERNEL_VERSION)/.config
	rm -rf ./$(KERNEL_VERSION)
	mv .work/$(KERNEL_VERSION) .

#$(PATCH_SENTINEL): | $(KERNEL_BUILD_PATH)
#	for p in $(PATCH_FILES); do patch -p1 -d $(KERNEL_VERSION) < "$$p"; done
#	touch $@

$(PH_LINUX_VER): $(KERNEL_BUILD_OUTPUT)
	rm -f $(PH_LINUX)
	cp $(KERNEL_BUILD_OUTPUT) $(PH_LINUX_VER)
	strip $(PH_LINUX_VER)
	ln -s $(PWD)/$(PH_LINUX_VER) $(PH_LINUX)

$(KERNEL_BUILD_OUTPUT): $(KERNEL_BUILD_PATH)
	cd $(KERNEL_VERSION); make -j$(NPROC)

clean:
	rm -rf $(KERNEL_BUILD_PATH) $(SHA256_SENTINEL) $(PH_LINUX_VER) $(PH_LINUX) .work

$(SHA256_SENTINEL): $(KERNEL_DL_PATH)
	sha256sum -c v$(LINUX_VERSION).sha256
	touch $@

gen-sha256: $(SHA256_FILE)

GPGTMP = verify-tmp/
GPGOPTS = --homedir $(GPGTMP) --trust-model direct --no-autostart
GPG_IMPORT = gpg $(GPGOPTS) --import
GPG_VERIFY = gpg $(GPGOPTS) --verify

$(KERNEL_DL_PATH).verify: $(KERNEL_DL_PATH)
	rm -rf $(GPGTMP)
	mkdir -m 0700 $(GPGTMP)
	$(GPG_IMPORT) < keys/torvalds.key
	$(GPG_IMPORT) < keys/gregkh.key
	$(GPG_VERIFY) downloads/$(KERNEL_VERSION).tar.sign $(KERNEL_DL_PATH) 2> .out
	mv .out $(KERNEL_DL_PATH).verify
	rm -rf $(GPGTMP)

$(SHA256_FILE): $(KERNEL_DL_PATH).verify 
	sha256sum $(KERNEL_DL_PATH)  > v$(LINUX_VERSION).sha256

