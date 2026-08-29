# RUA

[🇺🇸 English](./README.md) · [🇷🇺 Русский](./README.ru.md)

RUA downloads the history of territorial control changes from DeepStateMap,
builds a trend forecast, and generates an interactive HTML report.

[View the published report](https://rua.hexq.ru)

[![CI](https://github.com/hexqnt/rua/actions/workflows/ci.yml/badge.svg)](https://github.com/hexqnt/rua/actions/workflows/ci.yml)
[![Deploy to Cloudflare Pages](https://github.com/hexqnt/rua/actions/workflows/deploy.yml/badge.svg)](https://github.com/hexqnt/rua/actions/workflows/deploy.yml)

## Install and run

RUA is a command-line application. It has one required option: the path to a
configuration file.

### Prebuilt binaries

Download the archive for your system from the
[latest GitHub release](https://github.com/hexqnt/rua/releases/latest):

| System | Archive |
| --- | --- |
| Linux x86_64 (most distributions) | [`rua-linux-x86_64-gnu.tar.gz`](https://github.com/hexqnt/rua/releases/latest/download/rua-linux-x86_64-gnu.tar.gz) |
| Linux x86_64 (musl, including Alpine) | [`rua-linux-x86_64-musl.tar.gz`](https://github.com/hexqnt/rua/releases/latest/download/rua-linux-x86_64-musl.tar.gz) |
| Windows x86_64 | [`rua-windows-x86_64-msvc.zip`](https://github.com/hexqnt/rua/releases/latest/download/rua-windows-x86_64-msvc.zip) |
| macOS Apple Silicon | [`rua-macos-aarch64.tar.gz`](https://github.com/hexqnt/rua/releases/latest/download/rua-macos-aarch64.tar.gz) |

Each archive contains the executable only. Download the example
[`config.toml`](./config.toml), place it in your working directory, and extract
the archive there.

On Linux or macOS, run:

```sh
./rua --config config.toml
```

On Windows PowerShell, run:

```powershell
.\rua.exe --config .\config.toml
```

### Install from source

Install the latest version from GitHub with a stable Rust toolchain:

```sh
cargo install --git https://github.com/hexqnt/rua --locked
rua --config config.toml
```

Cargo installs `rua` into its binary directory, which must be included in
`PATH`. The configuration file is not installed automatically; download
[`config.toml`](./config.toml) separately.

## Configuration

The repository includes a ready-to-use [`config.toml`](./config.toml). Its
`mode` field selects the operation:

| Mode       | Result                                                           |
| ---------- | ---------------------------------------------------------------- |
| `run`      | Download history, build a forecast, and generate the HTML report |
| `download` | Download history to CSV only                                     |
| `forecast` | Read a history CSV and write a forecast CSV                      |
| `render`   | Generate the HTML report from history and forecast CSV files     |

Relative paths in the configuration file are resolved from the directory in
which RUA is launched. With the example configuration, the complete report is
written to `dist/index.html`. The generated page loads Plotly and country flags
from a CDN, so it needs an internet connection when opened.

Set `archive_csv = true` to compress generated CSV files as `.csv.gz`. Chart,
forecast, and output settings can be adjusted in their corresponding sections
of the configuration file.

## Library use

The `rua::deepstatemap` module provides a typed asynchronous client for loading
DeepStateMap history. To use it without the CLI and reporting dependencies:

```toml
[dependencies]
rua = { git = "https://github.com/hexqnt/rua", default-features = false }
```

Technical notes about the forecast model are available in
[`Model.md`](./Model.md) (in Russian).

## License

Licensed under either the [Apache License 2.0](./LICENSE-APACHE) or the
[MIT License](./LICENSE-MIT), at your option.
