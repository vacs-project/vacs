# 0004: Protocol 3.x grows additively; old clients must keep working

- Status: accepted
- Date: 2026-10-10, amended 2026-11-10 (full enum inventory, compatibility test,
  behavior rule, end of 3.0 support), accepted 2026-11-12 with #1266 (compatibility
  test without a source copy, version share from logs)
- Source: pre-planning session for #1266, #460 and #300 (no recording); compatibility
  facts traced in the code at main 7d51aba2 and rechecked at 7294ea13. The amendments
  were agreed by a maintainer in the review of 2026-11-10.

## Context

Protocol 3.0.0 shipped with ad-hoc conferences. Predefined conferences (#1266),
instantaneous access (#460) and mentoring (#300) follow, and none of them should cost
another breaking protocol change. The server is always deployed at the latest
version, so the only skew that matters is an older 3.x client talking to a newer
server and, over WebRTC, to newer peers.

How a 3.0 client reacts to things it does not know decides what is safe:

- Unknown fields are ignored. No type in `vacs-protocol` or `vacs-vatsim` uses
  `deny_unknown_fields`.
- Unknown `ServerMessage` types deserialize to `ServerMessage::Unknown` and are
  skipped (`vacs-signaling/src/client.rs`).
- Any server message that fails to deserialize is skipped and the connection stays
  up. A new enum variant without a fallback inside a known message therefore drops
  that whole message silently: a new `CallTarget` variant makes an invitation never
  ring, a new `StationChange` variant loses the whole `StationChanges` batch and
  leaves the station list stale, and anything unparseable in the profile loses the
  whole `SessionInfo`.
- Reason enums (`CallErrorReason`, `CallCancelReason`, `CallRejectReason`,
  `LoginFailureReason`, `ErrorReason`) have an untagged `Unknown` fallback.
- Every other enum a 3.0 client parses has none:
  - calls: `CallTarget`, `CallDropReason`
  - network: `StationChange`, `SessionProfile`, `ActiveProfile`
  - profile: `ProfileType`, `ProfileView`, `CustomButtonColor`, the untagged `GeoNode`
    and `DirectAccessPageContent`, the layout enums `JustifyContent`, `AlignItems`,
    `FlexDirection` and `GeoPageDividerOrientation`, and the client page enums
    `FrequencyDisplayMode` and `ClientGroupMode`
- The server knows each client's `Login.protocol_version`.

Parsing is only half of it. A 3.0 client has only ever met the message sequences the
3.0 server produced. A sequence that parses but never occurred in the field, such as
a fresh invite with several targets or a call roster with two clients on one
station, runs through client code that was never exercised.

## Decision

1. Every 3.x protocol change is additive: new optional fields
   (`#[serde(default, skip_serializing_if = ...)]`) and new message types only.
2. No new variant is added to an enum that a 3.0 client parses unless that enum has
   an `Unknown` fallback. Where an enum lacks one, the change is expressed as a new
   optional field instead. For the untagged enums this includes new shapes: a node or
   page content that matches none of the 3.0 shapes is a new variant.
3. A feature that cannot be expressed this way degrades on old clients to today's
   behavior (a key that rings normally, a roster shown as a plain conference), never
   to a dropped message.
4. New server behavior toward a 3.0 client is held to the same standard as new
   syntax. Before the server sends a 3.0 client a message sequence that 3.0 servers
   never produced, that sequence is pinned by a test against the 3.0 receive path.
   Where it cannot be pinned, or the test fails, the server gates the behavior on
   the client's `Login.protocol_version` and falls back to a sequence 3.0 knows.
5. Each feature release bumps the minor version of `VACS_PROTOCOL_VERSION`. The
   policy's `compatible_protocol_range` stays `^3.0.0`.
6. Rules 1 and 2 are enforced by a test, not only by review. `vacs-protocol` carries a
   test that serializes every `ServerMessage` the current crate can produce and
   deserializes it with the protocol types as released in 3.0.0. Adding a variant to
   any wire enum fails that test until the author has shown what a 3.0 client does
   with it.
7. Supporting 3.0 clients ends by decision, not by accident. The share of clients per
   protocol version is read from the server's `Client logged in` log events, which
   carry `protocol_version`. Once the share of clients below
   the current minor is negligible, the release policy's `required` range moves them
   to an update, and the workarounds a feature carries for older clients may be
   removed in the release after that.
8. Newer clients against an older server are out of scope: the server is always
   deployed first.

## Why (and what we rejected)

- **Another breaking bump for #460 or #300:** rejected by the maintainer; a 3.0 client
  must keep working against every 3.x server.
- **A `#[serde(other)] Unknown` on `ClientMessage`:** it would only help a newer client
  talking to an older server, which point 8 rules out.
- **Server-side capability negotiation:** not needed while every change is additive
  and the server knows the client's protocol version.
- **Review alone for rule 2:** the first version of this record already listed the
  fallback-less enums incompletely, and the e2e harness cannot run mixed client
  versions. A test against the released types is the only automated guard.
- **Supporting 3.0 for as long as 3.x lasts:** every later feature would keep paying
  for clients the updater can move. The promise is that a 3.0 client is never broken
  on the wire, not that it is never asked to update.

## Consequences

- #1266, #460 and #300 are each checked against these rules in ADRs 0005 to 0007.
- The compatibility test is the standalone crate `vacs-protocol/compat-3-0`, outside
  the main workspace with its own lockfile, run in CI with `--manifest-path`. It
  depends on the current `vacs-protocol` by path and on `vacs-protocol` at the
  `vacs-client-v3.0.0` tag by git. Inside the main workspace the git dependency made
  every `cargo -p vacs-protocol` ambiguous (two packages of that name and version);
  a verbatim copy of the 3.0.0 source was tried next and rejected by the maintainer
  as 3,000 lines of vendored code. New message types and reasons are entered in the
  test's `MESSAGES_ADDED_SINCE_3_0` and `REASONS_ADDED_SINCE_3_0` lists, which is
  where a reviewer sees what a 3.0 client will not understand.
- Rule 4 had one debt, the 3.0 receive path for a fresh multi-target invite. It is
  now pinned by `vacs-client/e2e/specs/incoming-multi-target-call.e2e.ts` (ADR 0005).
- A per-version client gauge was implemented with #1266 and dropped by the
  maintainer: the login log events already answer the question.
- Reviews of `vacs-protocol` changes still check for new variants on fallback-less
  enums (`protocol-compat-reviewer`); the test is the backstop.
- Release policy `required` ranges remain the lever for pushing old clients to update
  without breaking the wire.
- A server rollback below a released client's minor breaks point 8's assumption.
  Rolling the server back past a protocol minor is therefore not a supported recovery
  path once clients of that minor are out.
