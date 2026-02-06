.PHONY: build test benchmark update-baselines clean help release-patch release-minor release-major

# Default target
all: build

## build: Build the project in debug mode
build:
	cargo build

## test: Run all tests
test:
	cargo test

## benchmark: Run codegen benchmarks on fixtures
benchmark:
	cargo run -- benchmark tests/fixtures/codegen

## update-baselines: Update all test baseline files from current codegen output
update-baselines: build
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

## help: Show this help message
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@sed -n 's/^##//p' Makefile | column -t -s ':' | sed -e 's/^/ /'
