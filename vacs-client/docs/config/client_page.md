# Client Page configuration reference

> [!NOTE]  
> This configuration was previously known as the "Stations" configuration (in vacs 1.x). It has been renamed and restructured in vacs 2.0.0. If you are migrating from a previous version, see the [Migration Guide](#migrating-from-vacs-1x-stations-config) at the bottom of this document.

This reference explains how to configure client page filtering, prioritization, and display using the `ClientPageSettings`, read from the (optional) `client_page.toml` config file.

For general information on the configuration file format, file locations, and recommended editors, please refer to the [main configuration reference](README.md).

## Overview

The `client_page` configuration allows you to customize how clients are displayed and filtered while not using a dataset-defined profile. It consists of the following sections:

- **[Configs](#configs)** - Define (multiple) filtering configurations that you can switch between in the UI

> [!TIP]  
> You can load an additional client page configuration file (e.g., from a shared profile or sector file package) using the `extra_client_page_config` setting in your `client.toml` file.  
> See the [client configuration reference](client.md#extra-client-page-config) for more details.

> [!IMPORTANT]  
> These settings are purely client-side and do not prevent a different user from calling you, even if your filters do not match their callsign and you thus cannot see them.  
> If you are receiving a call from a station you cannot currently see, they will still have their respective callsign shown in the call display, however, you **will not** be able to call them back.

## Configuration structure

```toml
# Configs for filtering and prioritizing clients
[client_page.configs.Default]
include = []
exclude = []
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
frequencies = "ShowAll"
grouping = "FirAndIcao"
```

---

## Configs

Configs allow you to define multiple filtering configurations and switch between them in the UI. Each config controls which clients are shown and how they're ordered using three main settings:

- **`include`** – Allowlist patterns for clients to show
- **`exclude`** – Blocklist patterns for clients to hide
- **`priority`** – Ordered patterns that determine display order

### Config structure

```toml
# Define multiple configs under [client_page.configs.]
[client_page.configs.Default]
include = []
exclude = []
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
frequencies = "ShowAll"
grouping = "FirAndIcao"

[client_page.configs.CentersOnly]
include = ["*_CTR"]
exclude = []
priority = ["LOVV_CTR", "EDMM_CTR"]
frequencies = "HideAll"
grouping = "FirAndIcao"

[client_page.configs.LOVVOnly]
include = ["LO*"]
exclude = ["LON*"]
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
frequencies = "HideAll"
grouping = "Icao"
```

### Config names

Config names (the part after `client_page.configs.`) can contain:

- Letters (a-z, A-Z)
- Numbers (0-9)
- Underscores (`_`)
- Hyphens (`-`)

These names will be displayed in the UI for config selection.

### Config settings

Each config supports the following settings:

#### `include`: selecting which clients to show

**Type:** Array of strings ([glob patterns](#glob-pattern-matching))  
**Default:** `[]` (empty array)  
**Optional:** Yes

Controls which clients are eligible to be displayed.

- **If empty** (default): All clients are eligible, subject to `exclude` rules
- **If not empty**: Only clients matching at least one pattern are eligible, all other connected clients are hidden.

**Examples:**

```toml
[client_page.configs.local_area]
# Show only Austrian and Munich stations
include = ["LO*", "EDDM_*", "EDMM_*"]

[client_page.configs.app_ctr]
# Show only approach and center controllers
include = ["*_APP", "*_CTR"]

[client_page.configs.vienna]
# Show everything from Vienna airport
include = ["LOWW_*"]
```

---

#### `exclude`: hiding specific clients

**Type:** Array of strings ([glob patterns](#glob-pattern-matching))  
**Default:** `[]` (empty array)  
**Optional:** Yes

Excludes specific clients from being displayed. Exclude rules always take precedence over `include` rules, allowing you to e.g., include a whole FIR, but exclude all of their ground stations.

**Examples:**

```toml
[client_page.configs.hide_all_ground]
# Hide all ground, tower, and delivery positions
exclude = ["*_TWR", "*_GND", "*_DEL"]

[client_page.configs.hide_airports]
# Hide specific airports
exclude = ["LOWL_*", "LOWG_*"]

[client_page.configs.hide_fmp]
# Hide flow management positions
exclude = ["*_FPM"]
```

---

#### `priority`: ordering clients

**Type:** Ordered array of strings ([glob patterns](#glob-pattern-matching))  
**Default:** `["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]`  
**Optional:** Yes

Determines the display order of clients. The first matching pattern assigns the client's priority bucket – earlier patterns = higher priority.

Clients are grouped by their priority bucket and then sorted within each bucket. Clients that don't match any priority pattern appear last. After grouping, clients are sorted in alphabetical order (ascending) within their respective buckets.

**Default behavior:**

The default priority list orders clients by controller type:

1. Flow Management Positions (`*_FMP`)
2. Center controllers (`*_CTR`)
3. Approach controllers (`*_APP`)
4. Tower controllers (`*_TWR`)
5. Ground controllers (`*_GND`)

Clients not matched by the `priority` setting are grouped by their station type (alphabetical order, ascending), followed by the remaining clients _without_ a valid type (should only appear on `dev` server).

> [!TIP]  
> If you're trying to completely disable the default behavior, set `priority` to an empty array (`[]`).  
> If you omit the value from your config file, the default will be used.

**Examples:**

```toml
[client_page.configs.local_area]
# Prioritize your local area
priority = [
  "LOVV_*",           # Austrian center first
  "LOWW_*_APP",       # Vienna approach
  "LOWW_*_TWR",       # Vienna tower
  "LOWW_*",           # Other Vienna positions
  "*_CTR",            # Other centers
  "*_APP"             # Other approaches
]

[client_page.configs.centers_first]
# Simple setup: centers, then everything else (grouped by type, alphabetically, ascending)
priority = ["*_CTR"]
```

---

#### `frequencies`: controlling frequency display

**Type:** String (Enum)  
**Default:** `"ShowAll"`  
**Optional:** Yes

Controls how frequencies are displayed on the DA keys in the UI.

**Valid values:**

- `"ShowAll"` (default): Always show frequencies for all clients.
- `"HideAll"`: Never show frequencies for any client.

**Example:**

```toml
[client_page.configs.Compact]
# Hide all frequencies to save space
frequencies = "HideAll"
```

---

#### `grouping`: controlling client grouping

**Type:** String (Enum)  
**Default:** `"FirAndIcao"`  
**Optional:** Yes

Controls how DA keys are grouped.

**Valid values:**

- `"None"`: Don't group.
- `"Fir"`: Group by the first two letters (FIR) of the display name.
- `"FirAndIcao"` (default): First, group by the first two letters (FIR), then by the first four letters (ICAO code) of the display name.
- `"Icao"`: Group by the first four letters (ICAO code) of the display name.

**Example:**

```toml
[client_page.configs.GroupByFir]
# Group clients by FIR (e.g. LO, ED, ...)
grouping = "Fir"
```

##### Grouping Logic Examples

The following examples assume these clients are online:
`LOVV_CTR`, `LOWW_APP`, `LOWW_TWR`, `LOWW_GND`, `EDMM_CTR`, `EDDM_TWR`

**`grouping = "Fir"` or `"Icao"` (Single Layer)**

These modes create a simple one-level grouping structure.

**`Fir`**: Groups by the first two letters.

- **LO**
    - LOVV_CTR
    - LOWW_APP
    - LOWW_TWR
    - LOWW_GND
- **ED**
    - EDMM_CTR
    - EDDM_TWR

**`Icao`**: Groups by the first four letters.

- **LOVV**
    - LOVV_CTR
- **LOWW**
    - LOWW_APP
    - LOWW_TWR
    - LOWW_GND
- **EDMM**
    - EDMM_CTR
- **EDDM**
    - EDDM_TWR

**`grouping = "FirAndIcao"` (Two Layers)**

This mode creates a hierarchical structure, first grouping by FIR, then by ICAO code within that FIR.

- **LO**
    - **LOVV**
        - LOVV_CTR
    - **LOWW**
        - LOWW_APP
        - LOWW_TWR
        - LOWW_GND
- **ED**
    - **EDMM**
        - EDMM_CTR
    - **EDDM**
        - EDDM_TWR

---

### Glob pattern matching

All patterns use glob-like syntax, which provides flexible matching with wildcards:

#### Wildcards

- **`*`** – Matches zero or more characters
- **`?`** – Matches exactly one character

#### Matching rules

- Matching is **case-insensitive** (`loww` matches `LOWW`)
- Patterns must match the **entire callsign** (anchored at start and end)
    - If you want to match a substring in the middle, surround it with wildcards (e.g., `*WW*`)
- The pattern is converted to a regular expression where:
    - `*` becomes `.*` (any characters)
    - `?` becomes `.` (single character)
    - Other regex special characters are escaped

#### Pattern examples

| Pattern      | Matches                                | Doesn't Match             |
| ------------ | -------------------------------------- | ------------------------- |
| `LOWW_*`     | `LOWW_APP`, `LOWW_TWR`, `LOWW_1_TWR`   | `LOWWAPP`, `LOWI_APP`     |
| `*_APP`      | `LOWW_APP`, `EDDM_APP`, `LOVV_S_APP`   | `LOWW_TWR`, `APP`         |
| `LO*`        | `LOWW_APP`, `LOVV_CTR`, `LO123`        | `EDDM_APP`, `XLO`         |
| `LOWW*_APP`  | `LOWW_APP`, `LOWW_M_APP`, `LOWW_1_APP` | `LOWWAPP`, `LOWI_APP`     |
| `LOWW_?_TWR` | `LOWW_1_TWR`, `LOWW_2_TWR`             | `LOWW_TWR`, `LOWW_12_TWR` |
| `*`          | Everything                             | Nothing                   |
| `LOWW_APP`   | `LOWW_APP` (exact match)               | `LOWW_1_APP`              |

#### Common patterns

```toml
# All stations from a country prefix
include = ["LO*"]        # Austria (LOWW, LOWI, LOVV, etc.)
include = ["ED*"]        # Germany
include = ["LH*"]        # Hungary

# All positions at an airport
include = ["LOWW_*"]     # Vienna
include = ["EDDM_*"]     # Munich

# Specific position types everywhere
include = ["*_CTR"]      # All centers
include = ["*_APP"]      # All approaches
include = ["*_TWR"]      # All towers
include = ["*_GND"]      # All ground
include = ["*_DEL"]      # All delivery

# Numbered positions
include = ["LOWW_?_APP"] # LOWW_1_APP, LOWW_2_APP (single digit)
include = ["LOWW_*_APP"] # LOWW_1_APP, LOWW_12_APP (any number)

# Combined patterns
include = ["LOWW_*_TWR"] # All Vienna towers (but not LOWW_TWR)
include = ["ED*_CTR"]    # All German centers
```

---

### How filtering works

Clients are processed in this order:

1. **Include check**: If `include` is not empty, client must match at least one pattern
2. **Exclude check**: Client must not match any `exclude` pattern
3. **Priority assignment**: First matching `priority` pattern determines display order
4. **Display**: Clients are shown grouped and sorted by priority

#### Example walkthrough

Given this configuration:

```toml
[client_page.configs.example]
include = ["LO*", "EDMM_*", "EDDM_*"]
exclude = ["*_GND", "*_DEL"]
priority = ["LOVV*", "*_CTR", "LO*_APP", "*_APP", "*_TWR"]
```

Client processing:

| Callsign       | Include Match? | Exclude Match? | Priority       | Result                  |
| -------------- | -------------- | -------------- | -------------- | ----------------------- |
| `LOVV_CTR`     | ✓ (`LO*`)      | ✗              | 1 (`LOVV*`)    | **Shown, rank 1**       |
| `LOWW_APP`     | ✓ (`LO*`)      | ✗              | 3 (`*_APP`)    | **Shown, rank 3**       |
| `LOWW_GND`     | ✓ (`LO*`)      | ✓ (`*_GND`)    | –              | Hidden                  |
| `EDMM_ALB_CTR` | ✓ (`*_CTR`)    | ✗              | 2 (`*_CTR`)    | **Shown, rank 2**       |
| `EDDM_TWR`     | ✓ (`EDDM_*`)   | ✗              | 4 (`*_TWR`)    | **Shown, rank 4**       |
| `EDDF_APP`     | ✗              | ✗              | –              | Hidden (not in include) |
| `LON_S_FMP`    | ✓ (`LO*`)      | ✗              | 6 (no pattern) | **Shown, rank 5**       |

### Complete Examples

#### Example 1: Multiple workflow configs

Create different configs for different scenarios:

```toml
[client_page.configs.FIR_Wien]
# Only show clients from FIR Wien
include = ["LO*"]
exclude = ["LON*"]
priority = ["*_FMP", "*_CTR", "LOWW*_APP", "*_APP", "LOWW*_TWR", "*_TWR", "*_GND"]

[client_page.configs.CTR_only]
# Show only center controllers
include = ["*_CTR"]
exclude = ["*_FSS"]
priority = ["LOVV*", "EDMM*"]

[client_page.configs.No_Training]
# Hide common training positions
include = []
exclude = ["*_M_*", "*_X_*", "*_OBS"]
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
```

#### Example 2: FIR-specific configs

Create configs for different FIRs you control in:

```toml
[client_page.configs.LOVV]
include = ["LO*"]
exclude = ["LON*"]
priority = ["LOVV*", "LOWW*"]

[client_page.configs.EDMM]
include = ["EDMM*", "EDDM*"]
exclude = []
priority = ["EDMM*", "EDDM*"]
```

#### Example 3: Role-based configs

Create configs based on your controlling position:

```toml
[client_page.configs.TWR]
# When controlling tower, show relevant positions
include = ["LO*"]
exclude = ["LON*"]
priority = ["LOWW*_APP", "LOWW*_TWR", "LOWW*_GND", "LOWW*_DEL"]

[client_page.configs.CTR]
# When controlling center, focus on adjacent centers and approach
include = ["*_CTR", "*_APP"]
exclude = []
priority = ["LOVV*_CTR", "EDMM*_CTR", "*_CTR", "LOWW*_APP", "EDDM*_APP", "*_APP"]
```

### Tips

- Create multiple configs for different workflows (e.g., "Default", "CTR", "APP")
- Use descriptive config names that indicate their purpose
- Start with simple patterns and add complexity as needed
- Use `exclude` to refine broad `include` patterns
- Put your most important clients at the top of `priority`
- Leave `include` empty to see everything (filtered only by `exclude`)
- Remember that `exclude` always wins over `include`
- Use the `frequencies` option to toggle display of frequencies on your DA keys
- You can switch between configs in the UI without restarting the application

---

## Migrating from vacs 1.x (Stations config)

If you are upgrading from vacs 1.x to 2.0.0, the previous "Stations" configuration has been renamed and restructured into the "Client Page" configuration. This section explains how to migrate your existing `stations.toml` to the new `client_page.toml` format.

### Summary of changes

| Aspect                          | vacs 1.x                    | vacs 2.0.0                                                           |
| ------------------------------- | --------------------------- | -------------------------------------------------------------------- |
| Config file                     | `stations.toml`             | `client_page.toml`                                                   |
| Top-level key                   | `[stations]`                | `[client_page]`                                                      |
| Sub-sections                    | `stations.profiles.<name>`  | `client_page.configs.<name>`                                         |
| Config selection (client.toml)  | `selected_stations_profile` | `selected_client_page_config`                                        |
| Extra config file (client.toml) | `extra_stations_config`     | `extra_client_page_config`                                           |
| Default `grouping`              | `"None"`                    | `"FirAndIcao"`                                                       |
| `aliases`                       | Supported                   | **Removed**                                                          |
| `frequencies = "HideAliased"`   | Supported                   | **Removed** (use `"ShowAll"` or `"HideAll"`)                         |
| `[stations].ignored`            | Under `[stations]`          | Moved to `[client].ignored` (see [client.md](client.md#ignore-list)) |

### Step-by-step migration

#### 1. Rename the config file

Rename your `stations.toml` to `client_page.toml`.

#### 2. Update section headers

Replace all `[stations.profiles.<name>]` headers with `[client_page.configs.<name>]`:

**Before (1.x):**

```toml
[stations.profiles.Default]
include = []
exclude = []
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
```

**After (2.0.0):**

```toml
[client_page.configs.Default]
include = []
exclude = []
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
```

#### 3. Remove `aliases` sections

The `aliases` feature has been removed in 2.0.0. Delete any `[stations.profiles.<name>.aliases]` sections from your config.

**Before (1.x):**

```toml
[stations.profiles.FIC]
include = ["LO*"]
priority = ["*_CTR"]
frequencies = "HideAliased"

[stations.profiles.FIC.aliases]
"124.400" = "FIC_CTR"
"134.675" = "VB_APP"
```

**After (2.0.0):**

```toml
[client_page.configs.FIC]
include = ["LO*"]
priority = ["*_CTR"]
frequencies = "ShowAll"
```

#### 4. Update `frequencies` values

If you were using `"HideAliased"`, replace it with either `"ShowAll"` or `"HideAll"`:

- `"HideAliased"` → `"ShowAll"` (if you want to see frequencies) or `"HideAll"` (if you want to hide them)

#### 5. Review `grouping` default

The default value for `grouping` changed from `"None"` to `"FirAndIcao"`. If you were relying on the default (i.e., you did not explicitly set `grouping` in your config), your clients will now be grouped by FIR and ICAO code by default. If you prefer the old behavior, explicitly set:

```toml
grouping = "None"
```

#### 6. Move `ignored` to client.toml

If you had an `ignored` list under `[stations]`, move it to `[client]` in your `client.toml`:

**Before (1.x `stations.toml`):**

```toml
[stations]
ignored = ["10000003", "1234567"]
```

**After (2.0.0 `client.toml`):**

```toml
[client]
ignored = ["10000003", "1234567"]
```

#### 7. Update client.toml references

In your `client.toml`, update any references to the old setting names:

**Before (1.x):**

```toml
[client]
selected_stations_profile = "Default"
extra_stations_config = "/path/to/extra_stations.toml"
```

**After (2.0.0):**

```toml
[client]
selected_client_page_config = "Default"
extra_client_page_config = "/path/to/extra_client_page.toml"
```

### Full migration example

**Before (1.x `stations.toml`):**

```toml
[stations]
ignored = ["10000003"]

[stations.profiles.Default]
include = []
exclude = []
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
frequencies = "ShowAll"
grouping = "None"

[stations.profiles.FIR_Wien]
include = ["LO*"]
exclude = ["LON*"]
priority = ["*_FMP", "*_CTR", "LOWW*_APP", "*_APP"]
frequencies = "HideAliased"

[stations.profiles.FIR_Wien.aliases]
"124.400" = "FIC_CTR"
```

**After (2.0.0 `client_page.toml`):**

```toml
[client_page.configs.Default]
include = []
exclude = []
priority = ["*_FMP", "*_CTR", "*_APP", "*_TWR", "*_GND"]
frequencies = "ShowAll"
grouping = "None"

[client_page.configs.FIR_Wien]
include = ["LO*"]
exclude = ["LON*"]
priority = ["*_FMP", "*_CTR", "LOWW*_APP", "*_APP"]
frequencies = "ShowAll"
```

**And in `client.toml`, add:**

```toml
[client]
ignored = ["10000003"]
```
