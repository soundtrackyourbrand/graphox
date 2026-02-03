.PHONY: build test benchmark update-baselines clean help

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

## help: Show this help message
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@sed -n 's/^##//p' Makefile | column -t -s ':' | sed -e 's/^/ /'
