CC=gcc
SED=sed
CLANG_FORMAT=clang-format-3.9
CLANG_TIDY=clang-tidy-3.9
PREFIX = /usr
SYSCONFDIR = /etc
BINDIR = $(PREFIX)/bin
SRCFILES := sommelier.c version.h
XMLFILES := protocol/aura-shell.xml protocol/viewporter.xml protocol/xdg-shell-unstable-v6.xml protocol/linux-dmabuf-unstable-v1.xml protocol/drm.xml protocol/keyboard-extension-unstable-v1.xml protocol/gtk-shell.xml protocol/relative-pointer-unstable-v1.xml protocol/text-input-unstable-v1.xml protocol/pointer-constraints-unstable-v1.xml
AUXFILES := Makefile README LICENSE AUTHORS sommelier@.service.in sommelier-x@.service.in sommelierrc sommelier.sh
ALLFILES := $(SRCFILES) $(XMLFILES) $(AUXFILES)
#GIT_VERSION := $(shell git describe --abbrev=4 --dirty --always --tags)
#DIST_VERSION := $(shell git describe --abbrev=0 --tags)
DIST_VERSION_BITS := $(subst ., ,$(DIST_VERSION))
DIST_VERSION_MAJOR := $(word 1,$(DIST_VERSION_BITS))
DIST_VERSION_MINOR := $(word 2,$(DIST_VERSION_BITS))
DIST_VERSION_MINOR_NEXT := $(shell expr $(DIST_VERSION_MINOR) + 1)
CFLAGS=-g -Wall `pkg-config --cflags libdrm xcb xcb-composite xcb-xfixes wayland-server wayland-client gbm pixman-1` -I. -D_GNU_SOURCE=1 -DWL_HIDE_DEPRECATED=1 -DXWAYLAND_PATH=\"$(PREFIX)/bin/Xwayland\"
LDFLAGS=-lpthread -lm `pkg-config --libs libdrm xcb xcb-composite xcb-xfixes wayland-server wayland-client gbm pixman-1 xkbcommon`
DEPS = xdg-shell-unstable-v6-client-protocol.h xdg-shell-unstable-v6-server-protocol.h aura-shell-client-protocol.h viewporter-client-protocol.h viewporter-server-protocol.h linux-dmabuf-unstable-v1-client-protocol.h drm-server-protocol.h keyboard-extension-unstable-v1-client-protocol.h gtk-shell-server-protocol.h relative-pointer-unstable-v1-server-protocol.h relative-pointer-unstable-v1-client-protocol.h text-input-unstable-v1-client-protocol.h text-input-unstable-v1-server-protocol.h pointer-constraints-unstable-v1-client-protocol.h pointer-constraints-unstable-v1-server-protocol.h
OBJECTS = sommelier.o sommelier-compositor.o sommelier-data-device-manager.o sommelier-display.o sommelier-drm.o sommelier-gtk-shell.o sommelier-output.o sommelier-relative-pointer-manager.o sommelier-seat.o sommelier-shell.o sommelier-shm.o sommelier-subcompositor.o sommelier-text-input.o sommelier-viewporter.o sommelier-xdg-shell.o sommelier-pointer-constraints.o xdg-shell-unstable-v6-protocol.o aura-shell-protocol.o viewporter-protocol.o linux-dmabuf-unstable-v1-protocol.o drm-protocol.o keyboard-extension-unstable-v1-protocol.o gtk-shell-protocol.o relative-pointer-unstable-v1-protocol.o text-input-unstable-v1-protocol.o pointer-constraints-unstable-v1-protocol.o

#all: sommelier sommelier@.service sommelier-x@.service
all: sommelier

%.service: %.service.in
	$(SED) \
		-e 's|@bindir[@]|$(BINDIR)|g' \
		-e 's|@sysconfdir[@]|$(SYSCONFDIR)|g' \
		-e 's|@version[@]|$(DIST_VERSION)|g' \
		$< > $@

sommelier: $(OBJECTS)
	$(CC) $(OBJECTS) -o sommelier $(LDFLAGS)
	strip -s sommelier

%-protocol.c: protocol/%.xml
	wayland-scanner private-code < $< > $@

%-client-protocol.h: protocol/%.xml
	wayland-scanner client-header < $< > $@

%-server-protocol.h: protocol/%.xml
	wayland-scanner server-header < $< > $@

%.o: %.c
	$(CC) -c -o $@ $< $(CFLAGS)

$(OBJECTS): $(DEPS)

.PHONY: all install uninstall update-version dist deb version-clean clean style check-style tidy

install: all
	install -D sommelier \
		$(DESTDIR)$(PREFIX)/bin/sommelier
	install -D sommelierrc $(DESTDIR)$(SYSCONFDIR)/sommelierrc
	install -m 644 -D sommelier@.service \
		$(DESTDIR)$(PREFIX)/lib/systemd/user/sommelier@.service
	install -m 644 -D sommelier-x@.service \
		$(DESTDIR)$(PREFIX)/lib/systemd/user/sommelier-x@.service
	install -m 644 -D sommelier.sh $(DESTDIR)$(SYSCONFDIR)/profile.d/sommelier.sh

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/sommelier
	rm -f $(DESTDIR)$(SYSCONFDIR)/sommelierrc
	rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/sommelier@.service
	rm -f $(DESTDIR)$(PREFIX)/lib/systemd/user/sommelier-x@.service
	rm -f $(DESTDIR)$(SYSCONFDIR)/profile.d/sommelier.sh

update-version:
	dch -v $(DIST_VERSION_MAJOR).$(DIST_VERSION_MINOR_NEXT)-1
	git commit -m 'debian/changelog: bump to version $(DIST_VERSION_MAJOR).$(DIST_VERSION_MINOR_NEXT)' debian/changelog
	$(SED) -i -e 's/VERSION "[0-9.]*"/VERSION "$(DIST_VERSION_MAJOR).$(DIST_VERSION_MINOR_NEXT)"/g' version.h
	git tag $(DIST_VERSION_MAJOR).$(DIST_VERSION_MINOR_NEXT)

dist: $(DEPS)
	mkdir -p sommelier-$(DIST_VERSION)
	cp -r $(ALLFILES) $(DEPS) debian sommelier-$(DIST_VERSION)
	tar czf sommelier-$(DIST_VERSION).tar.gz sommelier-$(DIST_VERSION)
	rm -rf sommelier-$(DIST_VERSION)

deb: dist
	ln -sf sommelier-$(DIST_VERSION).tar.gz sommelier_$(DIST_VERSION).orig.tar.gz
	tar xzf sommelier-$(DIST_VERSION).tar.gz
	cd sommelier-$(DIST_VERSION) && debuild -i -us -uc -b
	rm -rf sommelier-$(DIST_VERSION) sommelier_$(DIST_VERSION).orig.tar.gz

clean:
	rm -f *~ *-protocol.c *-protocol.h *.o sommelier sommelier@.service \
		sommelier-x@.service sommelier-*.tar.gz sommelier*.deb \
		sommelier_*.build sommelier_*.buildinfo sommelier_*.changes

style: $(DEPS)
	@for src in $(SRCFILES) ; do \
		echo "Formatting $$src..."; \
		$(CLANG_FORMAT) -i "$$src"; \
		$(CLANG_TIDY) -checks='-*,readability-identifier-naming' \
			-config="{CheckOptions: [ \
			{ key: readability-identifier-naming.StructCase, value: lower_case  }, \
			{ key: readability-identifier-naming.FunctionCase, value: lower_case }, \
			{ key: readability-identifier-naming.VariableCase, value: lower_case }, \
			{ key: readability-identifier-naming.GlobalConstantCase, value: lower_case }, \
			{ key: readability-identifier-naming.EnumConstantCase, value: UPPER_CASE } \
			]}" "$$src"; \
	done
	@echo "Done"

check-style:
	@for src in $(SRCFILES) ; do \
		var=`$(CLANG_FORMAT) "$$src" | diff "$$src" - | wc -l`; \
		if [ $$var -ne 0 ] ; then \
			echo "$$src does not respect the coding style (diff: $$var lines)"; \
			exit 1; \
		fi; \
	done
	@echo "Style check passed"

tidy: $(DEPS)
	@for src in $(SRCFILES); do \
		echo "Running tidy on $$src..."; \
		$(CLANG_TIDY) -checks="-*,modernize-use-auto,modernize-use-nullptr, \
			readability-else-after-return,readability-simplify-boolean-expr, \
			readability-redundant-member-init,modernize-use-default-member-init, \
			modernize-use-equals-default,modernize-use-equals-delete, \
			modernize-use-using,modernize-loop-convert, \
			cppcoreguidelines-no-malloc,misc-redundant-expression" \
			"$$src"; \
	done
	@echo "Done"
