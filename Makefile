.PHONY: test coverage coverage-html clean release install

# Run all tests
test:
	cargo test

# Run tests with coverage (requires cargo-llvm-cov)
# Install: cargo install cargo-llvm-cov
coverage:
	cargo llvm-cov --all-features

# Generate HTML coverage report
coverage-html:
	cargo llvm-cov --all-features --html
	@echo "Coverage report generated at target/llvm-cov/html/index.html"

# Generate coverage report in lcov format (for CI/CD integration)
coverage-lcov:
	cargo llvm-cov --all-features --lcov --output-path lcov.info

clean:
	cargo clean
	rm -f lcov.info

BINS := $(patsubst src/bin/%.rs,%,$(wildcard src/bin/*.rs))
INSTALL_DIR := $(HOME)/.local/bin

release:
	cargo build --release

install: release
	mkdir -p $(INSTALL_DIR)
	@for bin in $(BINS); do \
		echo "Installing $$bin to $(INSTALL_DIR)"; \
		cp target/release/$$bin $(INSTALL_DIR)/; \
	done
