# opentrack

A modern, async CLI tool for tracking parcels across multiple carriers, written in Rust.

## Features

- **Multi-provider support** - Mondial Relay, La Poste / Colissimo (more planned)
- **Dual output modes** - pretty human-readable output or JSON
- **TUI mode** - interactive terminal UI for live tracking
- **Proxy support** - HTTP/HTTPS/SOCKS5 proxy configuration
- **XDG-compliant** - config and cache stored in standard XDG directories
- **Caching** - avoids redundant API calls with configurable TTL
- **Structured logging** - via `tracing` with configurable verbosity
- **Notification hooks** - desktop, webhook, ntfy, shell command, and more

## Installation

### From source

```sh
cargo install --path .
```

### Requirements

- Rust 1.75+
- CMake and Perl (required to compile BoringSSL used by `wreq`)
  ```sh
  # Debian/Ubuntu
  apt install cmake perl
  # Arch
  pacman -S cmake perl
  # macOS
  brew install cmake perl
  ```

## Usage

### Track a parcel

```sh
# Pretty output (default)
opentrack track --provider mondial-relay 12345678

# JSON output
opentrack track --provider mondial-relay 12345678 --json

# With postcode (required by some providers)
opentrack track --provider mondial-relay 12345678 --postcode 00000
```

### TUI mode

```sh
opentrack tui
```

Key bindings:

| Key              | Action                             |
| ---------------- | ---------------------------------- |
| `j` / `Down`     | Next parcel                        |
| `k` / `Up`       | Previous parcel                    |
| `J` / `PageDown` | Scroll events down                 |
| `K` / `PageUp`   | Scroll events up                   |
| `a`              | Add tracking entry                 |
| `d` / `Delete`   | Remove selected tracking           |
| `l`              | Toggle logs for selected tracking  |
| `[` / `]`        | Decrease/increase log filter level |
| `r`              | Refresh selected parcel            |
| `R`              | Refresh all parcels                |
| `q` / `Esc`      | Quit                               |
| `?`              | Help overlay                       |

### Manage saved parcels

```sh
# Add a parcel to your watchlist
opentrack add --provider mondial-relay 12345678 --label "My package"

# List all tracked parcels (with last known status from cache)
opentrack list
opentrack list --json

# Remove a parcel
opentrack remove 12345678
```

### Watch mode

Poll all saved parcels continuously and fire notifications on status changes:

```sh
opentrack watch                  # use interval from config (default: 300s)
opentrack watch --interval 60    # override poll interval in seconds
opentrack watch --once           # poll once then exit (useful for cron)
```

Run unattended with systemd:

```ini
[Unit]
Description=opentrack parcel watcher

[Service]
ExecStart=/usr/local/bin/opentrack watch
Restart=on-failure

[Install]
WantedBy=default.target
```

### Configuration

```sh
# Show config file path
opentrack config path

# Edit config (opens $EDITOR)
opentrack config edit
```

## Configuration

Config is stored at `$XDG_CONFIG_HOME/opentrack/config.toml` (typically `~/.config/opentrack/config.toml`).

```toml
[general]
# Default output format: "pretty" or "json"
output = "pretty"
# Cache TTL in seconds (0 to disable)
cache_ttl = 300
# Poll interval for watch mode in seconds
watch_interval = 300

[proxy]
# Optional proxy URL (takes precedence over env vars)
# url = "http://user:pass@proxy.example.com:8080"
# url = "socks5://proxy.example.com:1080"
# If unset, opentrack falls back to HTTP_PROXY/http_proxy
# (then HTTPS_PROXY/https_proxy, then ALL_PROXY/all_proxy).
# NO_PROXY/no_proxy is forwarded to Chromium in cdp mode via --proxy-bypass-list.
# In cdp mode, this is applied to auto-launched Chromium via --proxy-server.
# If you set cdp.endpoint, configure proxy on that browser process instead.

[cdp]
# Optional DevTools endpoint. If omitted in cdp mode, opentrack launches a temporary browser automatically.
# endpoint = "http://127.0.0.1:9222"
# Show launched browser window in cdp mode when opentrack starts its own browser (default: false)
show_browser = false
# Timeout for cdp operations in seconds
browser_timeout_secs = 25

[providers.mondial_relay]
# Optional override for country code (default: "fr")
country = "fr"
# Optional override for brand (default: "PP")
brand = "PP"
# Transport mode: "api" or "cdp" (default: "cdp")
mode = "cdp"

[providers.laposte]
# Optional language (default: "fr")
lang = "fr"

[[parcels]]
id = "12345678"
provider = "mondial-relay"
label = "My package"
postcode = "00000"
```

## Notifications

opentrack can fire notifications whenever a tracked parcel changes status. Notifications are evaluated by comparing the freshly-fetched state against the last cached state. Multiple notifiers can be active simultaneously.

### Triggers

Each notifier can be scoped to specific status transitions:

| Trigger         | Description                                       |
| --------------- | ------------------------------------------------- |
| `any`           | Any new tracking event                            |
| `status_change` | Status enum changes (e.g. InTransit -> Delivered) |
| `delivered`     | Parcel reaches Delivered status                   |
| `exception`     | Carrier reports an exception or problem           |

### Notifier types

#### Desktop (`desktop`)

Uses the system notification daemon via `libnotify` / D-Bus.

```toml
[[notifications]]
type = "desktop"
triggers = ["status_change", "delivered"]
# Optional: app name shown in notification center
app_name = "opentrack"
```

#### ntfy (`ntfy`)

Sends a push notification to a [ntfy](https://ntfy.sh) topic (self-hosted or ntfy.sh).

```toml
[[notifications]]
type = "ntfy"
triggers = ["delivered", "exception"]
url = "https://ntfy.sh/my-parcel-topic"
# Optional: ntfy access token
# token = "tk_mytoken"
# Optional: priority (min, low, default, high, urgent)
priority = "high"
```

#### Webhook (`webhook`)

HTTP POST to any URL with a JSON payload.

```toml
[[notifications]]
type = "webhook"
triggers = ["any"]
url = "https://hooks.example.com/parcel-update"
# Optional: additional headers
[notifications.headers]
Authorization = "Bearer mytoken"
X-Custom = "value"
```

Payload sent:

```json
{
  "parcel_id": "12345678",
  "provider": "mondial-relay",
  "label": "My package",
  "trigger": "status_change",
  "old_status": "InTransit",
  "new_status": "Delivered",
  "latest_event": {
    "timestamp": "2026-01-08T11:14:50Z",
    "description": "Colis livre au destinataire",
    "location": "CITY-TEST"
  }
}
```

#### Shell command (`command`)

Executes an arbitrary shell command. Notification context is passed as environment variables.

```toml
[[notifications]]
type = "command"
triggers = ["delivered"]
command = "notify-send 'Package delivered' '$OPENTRACK_LABEL ($OPENTRACK_ID)'"
```

Environment variables set:

| Variable                      | Value                                |
| ----------------------------- | ------------------------------------ |
| `OPENTRACK_ID`                | Parcel ID                            |
| `OPENTRACK_PROVIDER`          | Provider ID                          |
| `OPENTRACK_LABEL`             | Human label (or ID if unset)         |
| `OPENTRACK_TRIGGER`           | Trigger name                         |
| `OPENTRACK_OLD_STATUS`        | Previous status                      |
| `OPENTRACK_NEW_STATUS`        | New status                           |
| `OPENTRACK_EVENT_DESCRIPTION` | Latest event description             |
| `OPENTRACK_EVENT_TIMESTAMP`   | Latest event ISO 8601 timestamp      |
| `OPENTRACK_EVENT_LOCATION`    | Latest event location (may be empty) |

#### Matrix (`matrix`)

Sends a message to a Matrix room via the CS API.

```toml
[[notifications]]
type = "matrix"
triggers = ["status_change"]
homeserver = "https://matrix.example.org"
room_id = "!roomid:example.org"
access_token = "syt_mytoken"
```

### Per-parcel notification override

Notifications can be disabled or overridden per tracked parcel:

```toml
[[parcels]]
id = "12345678"
provider = "mondial-relay"
label = "My package"
postcode = "00000"
# Disable all notifications for this parcel
notify = false
```

## Cache

Cache is stored at `$XDG_CACHE_HOME/opentrack/` (typically `~/.cache/opentrack/`).

Clear cache:

```sh
opentrack cache clear
opentrack cache clear --provider mondial-relay
opentrack cache clear --id 12345678
```

## Providers

| Provider             | ID              | Notes                           |
| -------------------- | --------------- | ------------------------------- |
| Mondial Relay        | `mondial-relay` | Requires postcode in some cases |
| La Poste / Colissimo | `laposte`       | No authentication required      |

## Output example

```
Package 12345678 (Mondial Relay)
Destination: 00000

  Step 1  Colis en preparation chez l'expediteur       2026-01-02 17:32
  Step 2  Colis remis a Mondial Relay                  2026-01-02 17:54
  Step 3  Colis sur l'agence de livraison              2026-01-07 07:25
  Step 4  Colis disponible au point de retrait         2026-01-07 17:16
> Step 5  Colis livre au destinataire                  2026-01-08 11:14  [DELIVERED]

  Last event: Colis livre au destinataire
  Delivery point: LOCKER EXEMPLE - 10 RUE EXEMPLE, 00000 VILLE-TEST
```

## Logging

Control verbosity with `RUST_LOG`:

```sh
RUST_LOG=debug opentrack track --provider mondial-relay 12345678
RUST_LOG=opentrack=trace opentrack track --provider mondial-relay 12345678
```

## License

MIT
