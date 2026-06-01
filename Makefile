.PHONY: test coverage coverage-html clean release install sync-mt-assets

# Run all tests
test:
	cargo test

# ---- mt asset sync (Go → Rust mirror) ----
GO_STATIC    := ../internal/assets/static
RS_STATIC    := src/mt/assets/static

sync-mt-assets:
	@test -d $(GO_STATIC) || { echo "missing $(GO_STATIC) — run from funchain/ inside madtool"; exit 1; }
	rsync -a --delete --exclude '.gitkeep' $(GO_STATIC)/ $(RS_STATIC)/
	@echo "✓ Synced static assets from $(GO_STATIC) → $(RS_STATIC)"
	@echo "ℹ︎  template.html uses different syntax (minijinja vs Go html/template) — port manually if Go side changed."

# Run tests with coverage (requires cargo-llvm-cov)
# Install: cargo install cargo-llvm-cov
#
# Homebrew rustc 1.95+ doesn't ship llvm-tools in the path cargo-llvm-cov expects,
# so we point it at homebrew's llvm@22 directly. Override LLVM_COV / LLVM_PROFDATA
# on the command line if you're on a different setup.
LLVM_PREFIX  ?= /opt/homebrew/Cellar/llvm/22.1.6/bin
COVERAGE_ENV := LLVM_COV=$(LLVM_PREFIX)/llvm-cov LLVM_PROFDATA=$(LLVM_PREFIX)/llvm-profdata

coverage:
	$(COVERAGE_ENV) cargo llvm-cov --all-features --summary-only

# Generate HTML coverage report
coverage-html:
	$(COVERAGE_ENV) cargo llvm-cov --all-features --html
	@echo "Coverage report generated at target/llvm-cov/html/index.html"

# Generate coverage report in lcov format (for CI/CD integration)
coverage-lcov:
	$(COVERAGE_ENV) cargo llvm-cov --all-features --lcov --output-path lcov.info

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
