# Contributing

See [AGENTS.md](AGENTS.md) and the [provenance policy](docs/adr/0001-provenance.md).
Write code and fixtures independently; public decompilation is not permission to
copy/relicense game implementation. Credit format sources with immutable revision
links where available. Separate observed evidence from assumptions and fidelity
claims. Preserve Git author attribution.

## Keep private material private

Never attach or commit ROMs, extracted textures/geometry/audio, captured game
states/traces, emulator save states, game code, disassembly or decompiler output.
Do not download ROMs or modify the user's image. Use `.cache/`, `extracted/`,
`traces/` or other ignored runtime directories for local evidence. An ignore rule
is not protection against `git add -f` or an already tracked file.

Public JSON examples and regression fixtures must be **authored synthetic data**,
not captures renamed as examples. Compatibility addresses, canonical identity
hashes and aggregate counts may be documented with provenance. Describe the
revision, command, tool version, relevant backend and limitations needed to
reproduce evidence, not workstation usernames, personal paths, process IDs or
random local run IDs. Use `/path/to/your/rom.z64` and ignored output locations in
examples. Never post raw logs or screenshots without checking their contents.

## Checks (no ROM required)

Install stable Rust and the Linux build dependencies listed in [README.md](README.md).
Python tooling uses Python 3.11+ and its standard library only.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s scripts/tests -v
python3 -m unittest discover -s scripts/reference_emulator/tests -v
python3 scripts/check_publication.py
# Optional pre-publication audit of reachable local branches/tags:
python3 scripts/check_publication.py --history
```

The publication guard checks **tracked worktree files**, including unstaged edits,
not untracked/ignored output. Stage intended new source files before the final
check so they are included. It rejects private/generated paths, prohibited
artifact extensions, binary/non-UTF-8 contents, workstation profile paths and
obvious credential signatures. Authored JSON examples are explicitly permitted;
it cannot infer their provenance. `--history` additionally checks file names and
blob contents in all reachable commit trees, including deleted files. Neither
mode changes files, commits or author metadata. Git messages/author identities,
unreachable objects, LFS payloads and remote-only history are not audited.

This is a lightweight heuristic guard, **not** a license audit, comprehensive
secret scanner or proof that a text file contains no game-derived data. Review
`git diff --cached` and the intended publication history manually. Do not suppress
a real finding just to pass CI. For a credential leak, revoke/rotate it; for
private history, coordinate a separately reviewed history scrub while preserving
authorship. This checker does not rewrite history or publish anything.

## Optional private validation

Set `SM64_ROM` to your legally supplied US revision 0 image and run:

```sh
cargo test --workspace --locked -- --ignored --nocapture
```

These tests skip when the variable is unset; normal CI never acquires a ROM or
emulator. Keep resulting data local. Report reproducible commands, metadata and
limitations rather than including captured game data in a contribution.

The [static showcase](docs/showcase.md) can also be checked privately with a GPU.
Its `--shot` path needs no window/display server. Keep all generated PNGs and
logs ignored; no original-game screenshot is an authored regression fixture.
