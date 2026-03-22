# Contributing to vacs

Thanks for your interest in contributing to vacs! This guide will help you get a local development environment set up.

## Prerequisites

- **[Rust](https://rustup.rs/)** - edition 2024, MSRV **1.91.1**.
- **[Node.js](https://nodejs.org/)** - LTS (v24+), required for the Tauri frontend. We recommend [nvm](https://github.com/nvm-sh/nvm) for managing Node versions - an `.nvmrc` is provided in `vacs-client/`.

### Platform-specific dependencies

#### Linux (Debian/Ubuntu)

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
  libssl-dev libasound2-dev libgtk-3-dev libopus-dev \
  cmake patchelf
```

#### Linux (Fedora/RHEL)

```sh
sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel \
  librsvg2-devel openssl-devel alsa-lib-devel gtk3-devel opus-devel \
  cmake patchelf
```

#### macOS

Install [Homebrew](https://brew.sh/), then:

```sh
brew install opus
```

#### Windows

Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-studio-community/) (or the full Visual Studio IDE) with the **"Desktop development with C++"** workload selected. This provides the MSVC compiler, linker, and Windows SDK, which are required by Rust and Tauri.

No additional system libraries need to be installed manually -- the Rust build process handles everything else via `cargo`.

## Getting started

### Clone the repository

```sh
git clone https://github.com/vacs-project/vacs.git
cd vacs
```

### Install frontend dependencies

```sh
cd vacs-client
npm ci
```

This installs dependencies for both the `vacs-client` workspace root and the `frontend/` workspace.

### Building and running the client

> [!IMPORTANT]  
> Do not use `cargo build` directly to build the full desktop app. That only compiles the Rust backend and won't include the frontend.

To build and run the complete Tauri application (Rust backend + Preact frontend), use:

```sh
cd vacs-client
npm run dev
```

This runs `tauri dev`, which:

1. Starts the Vite dev server for the frontend (with hot-reload)
2. Compiles and launches the Rust/Tauri backend
3. Opens the app window pointing at the local dev server

For a production build including installer bundling, use:

```sh
cd vacs-client
npm run build
```

### Building individual Rust crates

If you're working on a backend crate that doesn't need the frontend (e.g., `vacs-server`, `vacs-audio`, `vacs-protocol`), you can use cargo directly:

```sh
cargo build -p vacs-server
cargo build -p vacs-audio
```

### Running the signaling server locally

The server requires a Redis-compatible key-value store for sessions. A Docker Compose file is included:

```sh
docker compose up -d   # starts Valkey (Redis-compatible) on port 6379
cargo run --bin vacs-server
```

The server reads its configuration from `config.toml` in the repository root.

## Development workflow

### Formatting

```sh
cargo fmt --all                         # Rust
cd vacs-client && npm run format        # Frontend (Prettier)
```

### Linting

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cd vacs-client && npm run -w frontend lint
```

### Testing

```sh
cargo test --locked --workspace --all-targets --all-features   # all Rust tests
cd vacs-client && npm test --workspaces                        # frontend tests
```

### Type checking the frontend

```sh
cd vacs-client && npm run -w frontend typecheck
```

## Project structure

```
vacs-audio/       Audio capture/playback, Opus codec, resampling
vacs-client/      Tauri desktop app (Rust backend + Preact frontend)
  frontend/       Preact/Vite/Tailwind frontend
vacs-macros/      Proc-macro crate
vacs-protocol/    Shared protocol types
vacs-server/      Axum signaling server
vacs-signaling/   WebSocket signaling layer
vacs-vatsim/      VATSIM API integration
vacs-webrtc/      WebRTC peer connection management
```

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the `vacs` project by you, as defined in the Apache-2.0 license, shall be dual-licensed under the MIT license and the Apache License, Version 2.0, at your option, without any additional terms or conditions.

In short: by contributing, you agree that your contributions may be used under either the MIT or the Apache-2.0 license, the same way as the existing code.

By submitting a contribution, you represent that you have the right to do so (e.g., your employer allows you to contribute under these terms) and that you are granting the project and its users a license to your contribution under the same terms.
