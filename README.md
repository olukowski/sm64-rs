# sm64-rs

An early project to reimplement **Super Mario 64** in Rust, using the user's own
ROM for content. No ROMs, Nintendo code, or game assets are included.

**Status:** workspace foundation; not a playable game. The first milestone is a
read-only ROM inspector, followed by a level viewer and a verified Mario simulation.
Bevy is planned for presentation; it is not needed by the format tools.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [architecture](docs/architecture.md), [roadmap](docs/roadmap.md), and
[provenance policy](docs/adr/0001-provenance.md).

## License

Original project code is licensed under MIT OR Apache-2.0.
Super Mario 64 and its content belong to Nintendo. This is an independent,
unofficial project, not affiliated with or endorsed by Nintendo.
See [LEGAL.md](LEGAL.md).
