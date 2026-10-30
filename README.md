# vacs - VATSIM ATC Communication System

[![Phare badge](https://img.shields.io/endpoint?url=https%3A%2F%2Fstatus.vacs.network%2Fshield-badges%2Fstatus.json&style=flat)](https://status.vacs.network)
[![Discord](https://img.shields.io/badge/Discord-chat-blue?style=flat&logo=discord)](https://discord.gg/yu2nyCKU3R)
[![Documentation](https://img.shields.io/badge/docs-docs.vacs.network-blue?style=flat)](https://docs.vacs.network)

**vacs** <small>([/vɐks/](https://ipa-reader.com/?text=v%C9%90ks&voice=Brian))</small> is an open-source, cross-platform **Ground-To-Ground Voice Communication System for VATSIM**, meant to provide a seamless coordination experience for virtual air traffic controllers.

We aim to modernize VATSIM controller-to-controller coordination by providing a low-latency and easy-to-use voice communication system.

<table>
  <tr>
    <td width="50%"><img src="https://docs.vacs.network/img/interface/geo_incoming_call.gif" alt="Geo layout with an incoming call flashing on a sector key"></td>
    <td width="50%"><img src="https://docs.vacs.network/img/interface/tabbed_outgoing_call.png" alt="Tabbed layout with an outgoing call ringing at a neighboring sector"></td>
  </tr>
  <tr>
    <td align="center">Geo layout with an incoming call</td>
    <td align="center">Tabbed layout with an outgoing call</td>
  </tr>
  <tr>
    <td width="50%"><img src="https://docs.vacs.network/img/interface/split_view_mixed.png" alt="Split view with the TrackAudio radio page next to the direct access keys"></td>
    <td width="50%"><img src="https://docs.vacs.network/img/settings/overview-plain.png" alt="Settings page with audio volumes, device selection and the configuration sub pages"></td>
  </tr>
  <tr>
    <td align="center">Split view with the radio integration</td>
    <td align="center">Settings</td>
  </tr>
</table>

## Features

- High-quality, low-latency voice communication using the [Opus codec](https://opus-codec.org/)
- Peer-to-peer audio streaming using [WebRTC](https://webrtc.org/) (no need for a centralized TeamSpeak/Discord or other voice server)
- Simple authentication using [VATSIM Connect](https://vatsim.dev/services/connect/) (no need to provide your VATSIM credentials)
- Sector-based call routing driven by the community-maintained [vacs-data](https://github.com/vacs-project/vacs-data) dataset, with geo and tabbed layouts per FIR
- Priority calls, conference calls, and a telephone directory for stations outside your layout
- Integration with [TrackAudio](https://github.com/pierr3/TrackAudio) and [Audio For VATSIM](https://audio.vatsim.net/), including radio playback
- Remote control of the client from a browser on another device
- Cross-platform desktop client (Windows, Linux, macOS) using [Tauri](https://tauri.app/)
- UI/UX inspired by real-life equivalents

## Getting started

The **[vacs user manual](https://docs.vacs.network)** covers installing, configuring and using vacs. Good places to start:

- [Requirements](https://docs.vacs.network/getting-started/requirements) and [Installation](https://docs.vacs.network/getting-started/installation)
- [Initial setup](https://docs.vacs.network/using-vacs/initial-setup) and [Making a call](https://docs.vacs.network/using-vacs/making-a-call)
- [Known issues & limitations](https://docs.vacs.network/known-issues-limitations) and the [FAQ](https://docs.vacs.network/faq)
- [Configuration files](https://docs.vacs.network/advanced/configuration) and the [Remote control API](https://docs.vacs.network/advanced/remote-api)

Releases for Windows, Linux (`.deb`, `.rpm`, `.AppImage`) and macOS are available on the [releases page](https://github.com/vacs-project/vacs/releases/latest). On macOS, you can also install vacs from our [Homebrew tap](https://github.com/vacs-project/homebrew-tap):

```bash
brew install --cask vacs-project/tap/vacs
```

All releases are signed. See [release verification](docs/release-verification.md) for how to check the signatures.

## Support

If you encounter any issues or have questions about `vacs`, please use one of the following channels:

- **Bug reports**: [open an issue](https://github.com/vacs-project/vacs/issues/new?template=bug_report.md) on our [issue tracker](https://github.com/vacs-project/vacs/issues) using the [`Bug report` template](https://github.com/vacs-project/vacs/issues/new?template=bug_report.md)
- **Feature requests**: check out our [project status board](https://github.com/users/vacs-project/projects/2) and [open an issue](https://github.com/vacs-project/vacs/issues/new?template=feature_request.md) on our [issue tracker](https://github.com/vacs-project/vacs/issues) using the [`Feature request` template](https://github.com/vacs-project/vacs/issues/new?template=feature_request.md)
- **Questions & discussions**: start a conversation on our [discussion forum](https://github.com/vacs-project/vacs/discussions)
- **System Status**: check our [status page](https://status.vacs.network) for real-time uptime monitoring and incident updates

Please search existing issues and discussions before creating a new one to avoid duplicates.

> [!TIP]  
> **Need help or just want to hang out?**  
> If you have questions, need support, or simply feel like chatting, check out our [Discord server](https://discord.gg/yu2nyCKU3R)!

## Roadmap

We track planned features, improvements, and known issues in our [GitHub Projects board](https://github.com/users/vacs-project/projects/2). Feel free to check there for what's coming next or to suggest new features.

## Privacy

**tl;dr** No audio is recorded, no credentials are stored, and calls are peer-to-peer. Your IP address is visible to call participants.

vacs only processes your VATSIM CID, never your password or other account details. Audio flows directly between call participants and never passes through the signaling server; when a direct connection is impossible, it is relayed through a TURN server operated by the core maintainers. The full [Privacy & Data Handling Policy](https://docs.vacs.network/legal/privacy-policy) describes what the official servers log and for how long.

## Signaling server

The `vacs` project provides a centralized signaling server (required for establishing calls), so no self-hosting is required. This server is used to track online stations and allow controllers to establish calls with their neighbors.  
As such, connecting to the default server is recommended to ensure the best possible reachability across the network.

However, we still provide the required server binaries in the form of a [Docker image](https://github.com/vacs-project/vacs/pkgs/container/vacs-server) should you prefer to host your own instance. Note that clients must manually edit their config files to specify your custom server URLs to use it and won't be able to call any other clients connected to the default server.

> [!NOTE]  
> Docker images for versions before 2.0.0 were published at `ghcr.io/morpheusxaut/vacs-server`. Starting with 2.0.0, images are published at `ghcr.io/vacs-project/vacs-server`.

### Production environment

By default, all released `vacs` clients will connect to the production server located at `https://vacs.network` (with the WebSocket endpoint available at `wss://vacs.network/ws`). This server will always run the latest stable version available.

### Development environment

In addition to the production server, we also provide a development server running on `https://dev.vacs.network` (`wss://dev.vacs.network/ws`) that can be used to test new changes before deploying them to production. Furthermore, the dev server does not require live VATSIM credentials but uses the [VATSIM Connect Sandbox environment](https://vatsim.dev/services/connect/sandbox) and its associated credentials. No active VATSIM connection is required to connect to the dev server.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for setting up a development environment, building the client and server, and running the test suites. Architectural decisions are recorded in [docs/decisions](docs/decisions/README.md), and the release process is described in [docs/release-automation.md](docs/release-automation.md).

The `vacs` project is a [Rust](https://rust-lang.org/) monorepo containing several crates:

- `vacs-client`: Cross-platform desktop client using [Tauri](https://tauri.app/) and [Preact](https://preactjs.com/)
- `vacs-server`: Axum-based signaling server providing HTTP and websocket API
- `vacs-protocol`: Shared protocol and types between client and server
- `vacs-audio`: CPAL audio backend including Opus encoder/decoder, a waveform generator and a basic mixer and DSP
- `vacs-webrtc`: WebRTC peer connection abstraction including protocol handling
- `vacs-signaling`: Websocket signaling protocol implementation and abstractions
- `vacs-vatsim`: Various VATSIM-specific client implementations and utilities used by both client and server
- `vacs-macros`: Internal macros used by various crates

While these crates were primarily developed to be used in our project's own client, they are also usable independently and aim to provide some abstractions to reduce developer load when implementing a custom client.

## Core Maintainers

The core maintainers are responsible for the project's direction, architecture decisions, release management, and the operation of all infrastructure provided by the vacs project (including the official signaling servers, website, and CI/CD pipelines). While vacs benefits from contributions by all of its community members, the core maintainers hold final decision-making authority on project scope, roadmap, and policy.

The current core maintainers are:

- **Luis Kreihsl** ([@IamLuisAUT](https://github.com/IamLuisAUT), VATSIM CID 1487671)
- **Nick Müller** ([@MorpheusXAUT](https://github.com/MorpheusXAUT), VATSIM CID 1461002)

## License

The `vacs` project and all its crates and packages are dual-licensed as

- **MIT License** ([LICENSE-MIT](https://github.com/vacs-project/vacs/blob/main/LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](https://github.com/vacs-project/vacs/blob/main/LICENSE-APACHE))

at your option.

This means you can choose to use `vacs` under either the MIT license or the
Apache-2.0 license.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project is provided under the same dual-license terms.
