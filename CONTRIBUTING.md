# Contributing

Keep changes focused and preserve the two-direct-dependency limit unless a new dependency has been discussed first.

Add one failing test for one behavior before changing its implementation.

Make the smallest implementation change that passes the test, then refactor while the suite stays green.

Run the local checks in continuous-integration order.

```console
cargo build --all-targets
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
nix fmt -- --check flake.nix package.nix
statix check .
deadnix --fail .
```

Use short imperative commit subjects and keep behavior changes separate from unrelated refactoring.
