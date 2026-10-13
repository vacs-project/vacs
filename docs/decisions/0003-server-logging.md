# 0003: Server logs are flat JSON events at full detail, queried by client and call

- Status: accepted
- Date: 2026-10-06
- Source: maintainer review of the production logs in Loki, 2026-10-06 (vacs-server 2.4.0)

## Context

Server logs rarely help when a user reports a problem, so we ask users for their client
log files instead. One day of production logs:

| Level | Lines | Bytes |
| --- | --- | --- |
| TRACE | 259,777 | 172 MB |
| DEBUG | 31,704 | 14 MB |
| WARN | 508 | 0.26 MB |
| ERROR | 16 | 0.01 MB |
| INFO | about 22 | - |

- The levels are inverted. Production runs our crates at TRACE, and what matters is
  logged at DEBUG or TRACE, while almost nothing is INFO.
- Three per-client, per-sync-tick messages make up 57% of all lines.
- Every line repeats its context. The JSON formatter writes the span stack twice, every
  nested `#[instrument]` adds its arguments again, and the client IP is on every line
  of a connection. A typical line is 660 bytes around a 30-byte message. Loki extracts
  only the innermost span, so the outer context cannot be queried anyway.
- `#[instrument(err)]` on nested functions logs one failure once per layer.
- WARN is mostly normal operation, such as client network drops.
- The events needed for a report are missing or buried: `call_id` is logged only at
  TRACE and ERROR, login outcomes at DEBUG or TRACE.
- Span arguments carry more than intended: storage keys that embed login and API
  tokens, and full request URIs including query strings.

## Decision

1. **Level policy.**

   | Level | Meaning |
   | --- | --- |
   | ERROR | A human has to act. |
   | WARN | The server hit something unexpected on its own side and recovered. |
   | INFO | One line per state change that matters to a user or an operator. |
   | DEBUG | The reasoning behind a decision. |
   | TRACE | Per-message and per-iteration detail. |

   Client network drops, pong timeouts and client protocol errors are INFO with a
   reason. INFO covers at least: startup and reloads; websocket connect and disconnect
   with reason and duration; login success and every rejection with its reason; invites
   rejected before a call exists; every call state change with `call_id`, reason and,
   on end, duration; VATSIM syncs that changed something; completed HTTP requests
   except health checks, `/favicon.ico` and the websocket upgrade request, which the
   connect line covers. The other routes under `/ws` are logged.

   Production keeps writing full detail for our own crates. The built-in filter
   becomes `info,vacs_=trace,vacs_vatsim::coverage=debug,axum::rejection=trace,axum_login=warn,tower_sessions=warn`.
   axum-login and tower-sessions wrap every handler in an INFO span of their own,
   which would repeat the CID as `user.id` and replace `name` on every request line.
   The level decides what is shown by default when reading, not what is written.

2. **Spans mark entry points only:** one per websocket connection, HTTP request,
   background task tick, and handled call message. They are INFO spans, so their
   fields survive the built-in filter and any override that keeps its `info` base.
   Internal functions get no `#[instrument]`.

   | Field | Meaning |
   | --- | --- |
   | `conn_id` | UUID v7 per websocket connection, never derived from IP or CID |
   | `client_id` | the CID, recorded on a connection as soon as the login token resolves, and on a request once the session authenticates it |
   | `call_id` | recorded after the server has checked the sender is a party to the call: caller, invitee or participant |
   | `peer_ids` | the CIDs of the other clients an event concerns, sorted and comma-separated, never the sender |
   | `task` | background task name |
   | `method`, `path` | HTTP request, never the query string |
   | `kind` | type of a protocol message, by its metric label |
   | `bytes` | size of a message in bytes |
   | `reason` | snake_case name of a reason enum variant, by its metric label |
   | `duration_ms` | duration of a request, session or call in milliseconds |
   | `client_ip` | the client's network, `/24` or `/48`, on connect and request completion lines only |
   | `status` | HTTP status code of a completed request |

   Values use Display formatting. An event never repeats a field an enclosing span
   carries, and `timestamp`, `level`, `target`, `message` and `name` are reserved for
   the formatter.

3. **Each error is logged once,** by the code that decides what happens next.
   `#[instrument(err)]` is not used.

4. **Formatter:** `json-subscriber` with event and span fields flattened to the top
   level, no span list, and the `tracing-log` feature so `log`-based dependencies
   share the shape. `name` is the innermost span:

   ```json
   {"timestamp":"2026-10-06T14:07:39.328Z","level":"INFO","target":"vacs_server::ws::handler",
    "name":"connection","message":"Client disconnected",
    "conn_id":"01928c5e-8d2a-7f7e-9c41-2e6a7d9b0f13","client_id":"1255177",
    "reason":"pong_timeout","duration_ms":5412318}
   ```

5. **Background tasks log on change only.** A tick that changed nothing writes at most
   one DEBUG line. Per-client iteration lines are dropped. Counts and durations go to
   Prometheus.

6. **Loki and Grafana.** Alloy promotes `conn_id`, `client_id`, `peer_ids` and `call_id`
   to structured metadata, never to labels, and never promotes the client IP. All
   levels are kept for 14 days. A Grafana dashboard starts from a `client_id`,
   `call_id` or `conn_id` and shows INFO and above by default. A `client_id` search
   also matches the CID inside `peer_ids`, so conference events are found at INFO.

7. **Never logged, at any level:**
   - tokens and credentials: login and API tokens, session ids, OAuth codes and
     responses, TURN credentials, config secrets, and storage keys or connection URLs
     that embed any of them
   - HTTP headers, bodies and query strings; single non-personal values may be logged as
     fields, such as the client version and channel of an update check
   - client-chosen strings in full: they are logged only after validation, or cut to 64
     bytes
   - SDP and ICE candidates: relayed WebRTC messages log `kind`, `call_id` and `bytes` only
   - whole protocol messages by Debug

   The client IP is truncated to its /24 (IPv4) or /48 (IPv6) network, after reducing
   IPv4-mapped and 6to4 addresses to IPv4. It appears only on connect and HTTP
   completion lines. Rate limiting keeps using the full address in memory.

## Why (and what we rejected)

- **Production at INFO, raising the level when needed.** There is a single instance
  with users around the clock, so a restart is not an option, and a report can only be
  investigated from what was already written.
- **Shorter retention for DEBUG and TRACE.** The volume at full detail is manageable;
  the problem is finding the right lines. Once point 7 applies, detail lines carry no
  personal data that INFO lines do not.
- **An in-memory ring buffer of recent detail.** Lost on every deploy or crash, which
  is when it is needed most.
- **Changing the filter at runtime.** It only helps once a problem is known. It can
  still be added later.
- **Spans as breadcrumbs on every function.** A JSON log does not render the span
  tree; it repeats it in full on every line.
- **Client network drops at WARN.** They are normal on home connections. WARN stays
  reserved for the server's own problems, so a WARN count means something.
- **Full client IPs.** Without SDP and ICE in the logs, the full address has no
  debugging value, and rate limiting works on it in memory anyway. The prefix still
  shows which network a user was on.
- **tracing-subscriber's JSON layer.** It cannot flatten span fields, and Loki cannot
  query the nested span list.
- **A custom formatter.** Code we would own for output `json-subscriber` already
  produces. The crate is MIT licensed and its dependencies are already in our lock
  file.
- **OpenTelemetry traces.** A new service on vacs-mgmt for questions a `call_id`
  filter answers.
- **Separate `peer_id` and `peer_ids` fields.** Two names for one concept, and every
  search would have to check both. A one-element list costs a regex on lookup, which
  is negligible at our volume. A JSON array is not an option: tracing field values
  and Loki structured metadata are both plain strings.
- **Durations in seconds for sessions and calls.** One unit and one field for every
  duration keeps queries and panels uniform; seconds are too coarse for requests.
- **Identifiers as Loki labels.** One stream per client or call is the cardinality
  problem Loki warns against.

## Consequences

- `vacs-server` adds `json-subscriber`, taking the newest release that is at least a
  week old.
- Every log statement in `vacs-server` is reviewed against points 1, 2 and 7 in one
  change, together with the formatter switch. HTTP completion lines get their own
  middleware, since tower-http's hooks cannot see the client IP and log 5xx twice.
- Tests pin the formatter output and the IP truncation. A test layer fails the
  integration suite on shadowed or reserved fields, Debug-formatted identifiers, the
  sender in its own `peer_ids`, an IP address outside the two lines that carry the
  truncated one, values over 2 KiB, and test tokens, SDP or ICE candidates in a line. With TRACE on in production, it enforces point 7.
- The Alloy pipeline reads only `level` from a line, which keeps its place, so the
  deploy does not break ingestion. Promoting the identifiers to structured metadata
  ships with it. No Grafana panels or alerts exist yet.
- Dependencies now log at INFO; noisy ones get their own directive after the first
  deploy. axum-login and tower-sessions already have one, for their request spans.
- Point 7 holds only under the built-in filter. `RUST_LOG` replaces it, and dependencies
  below INFO write what point 7 forbids: tungstenite at TRACE prints whole websocket
  frames, login tokens and SDP included, and hyper and reqwest at DEBUG print request
  URLs, which carry the Cloudflare key id. An override in production keeps
  `tungstenite`, `tokio_tungstenite`, `hyper`, `hyper_util`, `reqwest` and `h2` at INFO
  or above.
- An erasure request is served with Loki delete requests for lines that carry the CID
  as `client_id` or inside `peer_ids`, and for the connect lines of that client's
  `conn_id`s, which carry the IP but are written before login. The procedure lives in
  vacs-infra.
- Truncated IPs cannot reliably single out a user (an IPv6 /48 sometimes can), so the
  logs do not support network-level exclusion under the Terms of Use. Exclusion is out
  of scope here.
- The privacy policy gains a section on server logs and ships with 3.0.0. It is
  published with the deploy that stops logging tokens, together with a Loki delete
  request for the existing token lines. `README.md` changes with it.
- Handing `conn_id` to the client, so client logs can point to server lines, is a
  protocol change and needs its own record. `conn_id` stays a correlation id and is
  never accepted as a credential.
