# renametui

`renametui` is a Ratatui application for previewing and applying regex-based file and directory renames.

It never traverses a directory argument.

It changes only the final path component of each selected entry.

## LLM Policy

Caveat emptor: This project was almost entirely vibe-coded with ChatGPT 5.6.
In spite of this, I generally do not care for LLM-generated or LLM-assisted contributions.
Please divulge LLM involvement in any communication or code, and note that issues, PR, or other contributions may (or may not) be closed on this basis alone, with or without additional feedback from me.

## Features

- Accepts explicit file and directory paths, or defaults to every immediate entry in the current directory.
- Shows live before-and-after previews for a Rust regular expression and replacement string.
- Highlights only the regex-matched portions of each before name in green.
- Supports numbered and named replacement captures such as `$1` and `${name}`.
- Switches between files, directories, and both with `F1`, `F2`, and `F3`.
- Marks conflicts in red and blocks confirmation while any conflict remains.
- Shows conflict details in a blocking dialog when an invalid plan is submitted.
- Warns in yellow when Unix mode bits or sticky-directory rules suggest that the current user may be unable to rename an entry.
- Requires a separate confirmation step before touching the filesystem.
- Orders acyclic rename chains in memory and applies them with direct filesystem renames.
- Treats swaps and other rename cycles as conflicts because they have no overwrite-free direct execution order.
- Rechecks each destination immediately before its direct rename and attempts to roll back completed renames after an error.

Files include regular files, symbolic links, sockets, FIFOs, and other non-directory entries.

Symbolic links to directories are treated as files because the link itself is renamed.

## Installation

Build with stable Rust 2024 edition tooling.

```console
cargo build --release
```

The resulting executable is `target/release/renametui`.

A Nix development shell is also included.

```console
nix develop
cargo build
```

`package.nix` is exposed as the default flake package.

The Nix package uses `Cargo.lock`, which should be generated and committed before running `nix build`.

## Usage

```console
renametui [--] [PATH ...]
```

With no paths, the application reads all immediate entries in the current directory, including hidden entries.

Use `--` before a path beginning with a dash.

```console
renametui -- --literal-filename
```

The pattern is applied to each selected basename, not to its parent path.

For example, the pattern `^(.+)-(\d{4})\.txt$` and replacement `${2}_${1}.md` preview `report-2026.txt` as `2026_report.md`.

## Keys

| Key | Action |
| --- | --- |
| `Tab` | Switch between the pattern and replacement fields. |
| `Enter` | Move to the replacement field, or open confirmation or invalid-plan details from that field. |
| `F1` | Select files and other non-directory entries. |
| `F2` | Select directories. |
| `F3` | Select both. |
| `Up` / `Down` | Scroll the preview. |
| `PageUp` / `PageDown` | Scroll the preview by ten rows. |
| `Ctrl-A` | Move to the start of the active input field. |
| `Ctrl-E` | Move to the end of the active input field. |
| `Ctrl-U` | Clear the active input field. |
| `Ctrl-R` | Open confirmation or invalid-plan details from either field. |
| `y` | Apply the plan from the confirmation dialog. |
| `n` / `Esc` | Cancel the confirmation dialog. |
| `Ctrl-Q` / `Ctrl-C` | Quit. |

## Conflict checks

Confirmation is blocked when any of these conditions is detected.

- Multiple sources would produce the same destination.
- A destination already exists and is not another selected source that can be renamed first.
- A replacement produces an empty name, `.` or `..`, a slash, or a NUL byte.
- A selected filename is not valid UTF-8 and therefore cannot be processed by the `regex` crate.
- The same source is included more than once.
- A selected directory contains another selected source.
- The rename graph contains a swap or longer cycle with no conflict-free direct execution order.

On macOS, destination comparisons also use a conservative lowercase basename key to catch common case-insensitive collisions.

The planner permits acyclic chains such as `a -> aa` and `aa -> aaa` because it can rename `aa` first.

It refuses swaps and longer cycles rather than creating temporary filesystem names.

The preview contains only type, before, and after columns.

Each non-empty regex match is green in the before column while unmatched text keeps the row's normal style.

Conflicting rows remain entirely red, and pressing `Enter` to submit an invalid plan opens the detailed conflict messages.

## Permission warnings

On Linux and macOS, the application obtains the current numeric user and group IDs with `id -u` and `id -G`.

It checks whether the relevant owner, group, or other mode bits grant both write and search access to each source parent directory.

It also checks the sticky-directory rule against the parent owner and source owner.

These checks are warnings rather than guarantees because ACLs, capabilities, read-only mounts, network filesystems, and concurrent permission changes can differ from mode-bit predictions.

Permission-warning details are shown in the confirmation dialog before any rename is applied.

## Filesystem safety boundary

Every destination is checked during planning, during execution preflight, and immediately before its direct rename.

Rust's portable standard library does not expose an atomic cross-platform no-replace rename operation.

A separate process can therefore still create a destination in the small interval between the final existence check and `std::fs::rename`.

The application minimizes that interval and never intentionally overwrites a destination observed by its checks.

Completed direct renames are rolled back in reverse order after a later failure when the original names remain available.

Callers requiring protection from hostile concurrent mutation need platform-specific kernel primitives.

## Development

Run the checks in the same order as continuous integration.

```console
cargo build --all-targets
cargo test --all-targets
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

The project has only two direct Rust dependencies: `ratatui` and `regex`.

Ratatui re-exports Crossterm under its default feature set, so no direct terminal-backend dependency is needed.

## License

This project is available under the MIT License.
