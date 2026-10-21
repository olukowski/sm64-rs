# sm64-rs

A behavior-faithful native reimplementation of **Super Mario 64** in Rust,
using a user-supplied ROM for content. No ROMs, Nintendo code, or game assets
are included. This is not an emulator.

**Status:** ROM inspection and MIO0 decoding work. There is no renderer or
playable game yet. Bevy is planned for presentation; it is not a dependency of
the format tools.

## Try the ROM inspector

Requires a recent stable Rust toolchain and a ROM you are legally entitled to use.
No ROM is bundled, searched for online, or downloaded.

```sh
# Point at your own legally supplied ROM.
export SM64_ROM="/path/to/your/rom.z64"

cargo run -p sm64x -- info
cargo run -p sm64x -- mio0 list --validate

# Decode the first MIO0 block in the recognized US revision 0 ROM.
# Creates .cache/mio0/00108a40.bin; refuses to overwrite existing files.
cargo run -p sm64x -- mio0 decode 0x108a40
```

Alternatively, pass `--rom "/path/to/your/rom"` before or after the command.
An explicit path overrides `SM64_ROM`. ZIP/7z containers are not supported;
select an already unpacked ROM.

### Commands

| Command | Purpose |
| --- | --- |
| `info` | Size, source byte order, canonical SHA-1/SHA-256, exact revision recognition, N64 header |
| `mio0 list` | Find signature candidates and validate their headers |
| `mio0 list --validate` | Additionally decode candidates to check their stream structure |
| `mio0 decode OFFSET [--out PATH]` | Decode one block into a **new** output file |

Use `--json` with any command for machine-readable output. Offsets accept decimal
or `0x`-prefixed hexadecimal. CLI errors go to stderr with a nonzero exit status.
A scan reports invalid/skipped candidates in its results; their presence is not
itself a command failure.

```sh
mkdir -p .cache/reports
cargo run -p sm64x -- info --json > .cache/reports/info.json
cargo run -p sm64x -- mio0 list --validate --json > .cache/reports/mio0.json
cargo run -p sm64x -- mio0 decode --help
```

Keep extracted content and reports in ignored `.cache/` or `extracted/`.
Decoding never overwrites an existing file, including symlinks, and never writes
to the source ROM.

### Identity and limitations

- Detects big-endian `.z64`, pair-swapped `.v64`, and word-reversed `.n64` from
  their magic bytes, **not their filename extensions**. Normalizes only in memory.
- All reported offsets and identity hashes refer to canonical big-endian bytes.
- Exact recognition currently covers **US revision 0**. Other N64 ROMs can be
  inspected generically; a title or region byte does not establish authenticity.
- CRC1/CRC2 are **stored header values, not recomputed/verified checksums**.
- MIO0 signatures are candidates, not a complete asset directory. Successful
  decoding does not identify a block's contents.
- ROM input is capped at 64 MiB. MIO0 output defaults to 16 MiB per block;
  `--max-output-mib 1..64` adjusts it. Scans additionally cap attempted decoded
  output at 256 MiB and report at most 4,096 candidates. All candidates are
  counted; truncation and skipped validation are explicit.
- JSON `encoded_size` is the extent actually read from a block, including gaps,
  not an aligned storage size or proof of the next block's location.
- No level-script interpretation, asset type identification, display-list
  rendering, boot checksum verification, or gameplay implementation yet.

The local reference check recognized the US ROM and successfully decoded all
79 MIO0 signature candidates. See [reference notes](docs/research/reference-rom.md).

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# Optional; selects the US reference ROM via SM64_ROM. Never runs in CI.
cargo test --workspace --locked -- --ignored --nocapture
```

Normal tests use authored synthetic data and require no ROM. The ignored local
reference test skips with a message when `SM64_ROM` is unset.

- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
- [N64 ROM format notes](docs/formats/n64-rom.md)
- [MIO0 format notes](docs/formats/mio0.md)
- [Provenance policy](docs/adr/0001-provenance.md)
- [Contributor/agent rules](AGENTS.md)

## License

Original project code is licensed under MIT OR Apache-2.0.
Super Mario 64 and its content belong to Nintendo. This is an independent,
unofficial project, not affiliated with or endorsed by Nintendo.
See [LEGAL.md](LEGAL.md).
