
RUST_BUILD_RELEASE = rust/target/release

RUST_PH = $(RUST_BUILD_RELEASE)/pH

CARGO = cargo
CARGO_OPTS = --manifest-path rust/Cargo.toml --release

KERNEL = kernel/ph_linux

CP = cp

.PHONY: all clean $(RUST_PH)

all: pH $(KERNEL)

$(KERNEL):
	$(MAKE) -C kernel/

pH: $(RUST_PH)
	$(CP) $(RUST_PH) pH

$(RUST_PH): 
	$(CARGO) build --bins $(CARGO_OPTS)

clean:
	rm -f pH
	$(CARGO) clean $(CARGO_OPTS)

