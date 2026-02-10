.PHONY: build test benchmark update-baselines clean help release-patch release-minor release-major
.PHONY: cargo-build cargo-test swc-build swc-test babel-build babel-test check fmt clippy bench-compile

# Default target
all: build

## cargo-build: Build core Rust crate
cargo-build:
	cargo build

## cargo-test: Run Rust tests
cargo-test:
	cargo test

## swc-build: Build SWC plugin (WASM + TypeScript)
swc-build:
	cd plugins/swc/node && pnpm run build:all

## swc-test: Run SWC plugin tests
swc-test:
	cd plugins/swc/node && pnpm test

## babel-build: Build Babel plugin
babel-build:
	cd plugins/babel && pnpm install && pnpm run build

## babel-test: Run Babel plugin tests
babel-test:
	cd plugins/babel && pnpm test

## build: Build core crate + all plugins
build: cargo-build swc-build babel-build

## test: Run all tests (core crate + plugins)
test: cargo-test swc-test babel-test

## benchmark: Run benchmark suit
benchmark:
	cargo bench --features bench

## update-baselines: Update all test baseline files from current codegen output
update-baselines: cargo-build
	./scripts/update_baselines.py

## clean: Clean build artifacts
clean:
	cargo clean
	rm -rf temp_gen_out

## release-patch: Bump patch version, commit, tag, and push
release-patch:
	@./scripts/release.sh patch

## release-minor: Bump minor version, commit, tag, and push
release-minor:
	@./scripts/release.sh minor

## release-major: Bump major version, commit, tag, and push
release-major:
	@./scripts/release.sh major

## check: Run all linting, formatting, tests, and bench compilation
check: fmt clippy test bench-compile

## fmt: Format code
fmt:
	cargo fmt

## clippy: Run clippy with all targets and features
clippy:
	cargo clippy --all-targets --all-features -- --D warnings

## bench-compile: Compile benchmarks (without running)
bench-compile:
	cargo build --features bench --benches

## help: Show this help message
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@sed -n 's/^##//p' Makefile | column -t -s ':' | sed -e 's/^/ /'
