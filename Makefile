.PHONY: help preflight publish test check build clean examples doc release-check release

help:
	@echo "Available targets:"
	@echo "  make check          - Run cargo check on all crates"
	@echo "  make test           - Run all tests"
	@echo "  make build          - Build all crates"
	@echo "  make doc            - Generate documentation"
	@echo "  make examples       - Run all examples"
	@echo "  make preflight      - Run all checks before publishing"
	@echo "  make release-check  - Dry-run release with cargo-release"
	@echo "  make release        - Release with cargo-release"
	@echo "  make publish        - Publish to crates.io manually"
	@echo "  make clean          - Clean build artifacts"

check:
	@echo "🔍 Checking all crates..."
	cargo check --all-targets --all-features

test:
	@echo "🧪 Running tests..."
	cargo test --all-targets --all-features
	cargo test --doc --all-features

build:
	@echo "🔨 Building all crates..."
	cargo build --all-features

doc:
	@echo "📚 Generating documentation..."
	cargo doc --all-features --no-deps --open

clean:
	@echo "🧹 Cleaning build artifacts..."
	cargo clean

EXAMPLES :=

examples:
	@if [ -z "$(EXAMPLES)" ]; then \
		echo "ℹ️  No examples defined yet"; \
	else \
		for name in $(EXAMPLES); do \
			echo "Running $$name example..."; \
			cargo run --example $$name --package version-migrate; \
		done; \
	fi

preflight: examples
	@echo "🚦 Running preflight checks for the entire workspace..."
	@echo ""
	@echo "1️⃣  Formatting code..."
	cargo fmt --all
	@echo ""
	@echo "2️⃣  Running clippy (auto-fix)..."
	cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings
	@echo ""
	@echo "3️⃣  Running tests..."
	cargo test --all-targets --all-features
	cargo test --doc --all-features
	@echo ""
	@echo "✅ All preflight checks passed!"

release-check:
	@echo "🔍 Dry-run release with cargo-release..."
	@echo ""
	@echo "Note: Install cargo-release if not already installed:"
	@echo "  cargo install cargo-release"
	@echo ""
	cargo release --workspace --exclude version-migrate-macro --dry-run

release: preflight
	@echo "🚀 Releasing with cargo-release..."
	@echo ""
	@echo "This will:"
	@echo "  - Update version numbers"
	@echo "  - Create git tags"
	@echo "  - Publish to crates.io in the correct order"
	@echo ""
	@read -p "Continue? [y/N] " confirm && [ "$$confirm" = "y" ] || exit 1
	cargo release --workspace --exclude version-migrate-macro --execute

# 手動公開（cargo-releaseを使わない場合）
publish: preflight
	@echo ""
	@echo "🚀 Starting sequential publish process..."
	@echo ""
	@echo "⚠️  Note: Consider using 'make release' with cargo-release instead"
	@echo ""

	@echo "--- Step 1: Publishing version-migrate-macro ---"
	@echo "  Running dry-run for version-migrate-macro..."
	cargo publish -p version-migrate-macro --dry-run --allow-dirty

	@echo "  ✓ Dry-run successful for version-migrate-macro"
	@echo "  Publishing version-migrate-macro to crates.io..."
	cargo publish -p version-migrate-macro --allow-dirty

	@echo ""
	@echo "✅ version-migrate-macro published successfully!"
	@echo ""
	@echo "⏳ Waiting 30 seconds for crates.io index to update..."
	sleep 30

	@echo ""
	@echo "--- Step 2: Publishing version-migrate ---"
	@echo "  Running dry-run for version-migrate..."
	cargo publish -p version-migrate --dry-run --allow-dirty

	@echo "  ✓ Dry-run successful for version-migrate"
	@echo "  Publishing version-migrate to crates.io..."
	cargo publish -p version-migrate --allow-dirty

	@echo ""
	@echo "✅ version-migrate published successfully!"
	@echo ""
	@echo "🎉 All crates have been successfully published to crates.io!"
