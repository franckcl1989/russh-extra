# russh-extra verification shortcuts
#
# Run `just` or `just check-all` for the full verification suite.
# Run `just fix` to auto-format before committing.

default: check-all

# Full verification suite (what CI runs)
check-all: fmt-check clippy test doc-check feature-checks

# Auto-fix formatting
fix:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all --check

# Compile check with all features
check:
    cargo check --workspace --all-targets --all-features

# Clippy with warnings as errors
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --workspace --all-features

# Build docs (no deps)
doc-check:
    cargo doc --workspace --all-features --no-deps

# Feature-gating checks
feature-checks:
    cargo check -p russh-extra --no-default-features
    cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features server,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features known-hosts,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features shell,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features tunnel,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features sftp,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features client,ring
    cargo check -p russh-extra --no-default-features --features server,sftp,aws-lc-rs
    cargo check -p russh-extra --no-default-features --features full

# Run default-feature tests only (faster)
test-default:
    cargo test --workspace

# Check with no default features
check-no-defaults:
    cargo check --workspace --no-default-features

# Package validation (pre-release)
package-check:
    cargo package --workspace --allow-dirty
