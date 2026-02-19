.PHONY: all fmt fmt-check build app dev check test install-tools test-webgpu-fallback

all: build

RUST_LOG ?= info

test:
	cargo nextest run
	cargo nextest run -p wezterm-escape-parser # no_std by default

check:
	cargo check
	cargo check -p wezterm-escape-parser
	cargo check -p wezterm-cell
	cargo check -p wezterm-surface
	cargo check -p wezterm-ssh

app:
	PROFILE=debug ./scripts/build.sh --app-only

dev:
	@if ! command -v cargo-watch >/dev/null 2>&1; then \
		echo "Installing cargo-watch..."; \
		cargo install cargo-watch --locked; \
	fi
	RUST_LOG=$(RUST_LOG) cargo watch \
		--skip-local-deps \
		-w Cargo.toml \
		-w kaku-gui \
		-w window \
		-w term \
		-w mux \
		-w config \
		-w kaku \
		-w lua-api-crates \
		-i "dist/**" \
		-i "deps/**" \
		-x "run $(BUILD_OPTS) -p kaku-gui --"

build:
	cargo build $(BUILD_OPTS) -p kaku -p kaku-gui -p wezterm-mux-server-impl

fmt:
	cargo +nightly fmt -p kaku -p kaku-gui -p mux -p wezterm-term -p termwiz -p config -p wezterm-font

fmt-check:
	cargo +nightly fmt -p kaku -p kaku-gui -p mux -p wezterm-term -p termwiz -p config -p wezterm-font -- --check
	@echo "Format check passed."

install-tools:
	cargo install cargo-nextest --locked
	cargo install cargo-watch --locked
	rustup toolchain install nightly --component rustfmt
	@echo "Tools installed."

test-webgpu-fallback:
	./scripts/test_webgpu_fallback.sh --strict
