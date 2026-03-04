# vacs configuration reference

This document provides a reference for the config file(s) used by `vacs` (client).

## Overview

The `vacs` client reads configuration from:

1. Built-in defaults,
2. `config.toml` in the config directory,
3. `config.toml` in the current working directory,
4. `audio.toml` in the config directory,
5. `audio.toml` in the current working directory,
6. `client_page.toml` in the config directory,
7. `client_page.toml` in the current working directory,
8. `client.toml` in the config directory,
9. `client.toml` in the current working directory,
10. Environment variables with the `VACS_CLIENT_` prefix.

The config directory is dependent on the operating system:

- Linux: `$XDG_CONFIG_HOME/app.vacs.vacs-client/` or `$HOME/.config/app.vacs.vacs-client/`
- macOS: `$HOME/Library/Application Support/app.vacs.vacs-client/`
- Windows: `%APPDATA%\app.vacs.vacs-client\`

Later sources override earlier ones. Whilst all config files _can_ contain any kind of configuration value,
vacs only persists a certain subset of configuration depending on the file read/written.

All configuration files use the [TOML](https://toml.io/en/) format.  
Various tools exist helping you create and edit TOML files, such as [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) for [Visual Studio Code](https://code.visualstudio.com/).
If your tool of choice supports [JSON Schema](https://json-schema.org/), you can find the schema for the `vacs` configuration in this directory ([config.schema.json](config.schema.json)) or as a [GitHub URL](https://raw.githubusercontent.com/vacs-project/vacs/refs/heads/main/vacs-client/docs/config/config.schema.json).

## Top-level structure

```toml
[backend]
# BackendConfig

[audio]
# AudioConfig

[webrtc]
# WebRTCConfig

[client]
# ClientConfig

[client_page]
# ClientPageSettings
```

### `backend`: Backend server configuration

### `audio`: Audio input/output settings

### `webrtc`: WebRTC call-related settings

### `client`: Influencing the client's behavior

[ClientConfig reference](client.md)

### `client_page`: Filtering and customizing the Client page

[ClientPageSettings reference](client_page.md)
