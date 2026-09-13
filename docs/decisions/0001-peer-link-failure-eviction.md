# 0001: Peer link failures evict the later joiner after relay-assisted retries

- Status: accepted
- Date: 2026-08-21, amended 2026-08-24 (retry budget, leader eviction, limbo
  handling, report freshness), implemented 2026-09-04 (protocol 3.0.0)
- Source: adhoc-conferences review session

## Context

Ad-hoc conferences are a full mesh of peer-to-peer WebRTC connections. A single link
can fail while every other link works: in a call between A, B and C, the pair A-B can
lose connectivity while A-C and B-C stay healthy (NAT changes, one-sided network
issues, firewall rules).

The protocol as of the 3.0.0 work can only express "client X errored"
(`CallErrorReason::WebrtcFailure(ClientId)`, validated to name the sender). The
server's only reaction is to remove the reporting client from the call outright, and
to end the call for everyone when that client is the conference leader
(`CallManager::call_error` falling through to `end_call`). One broken link therefore
destroys either the reporter's participation or the entire conference, and the
reporter is not even told (`handle_call_error` fans out only to the remaining
participants).

## Decision

A dead link between two conference participants is handled in four parts:

1. **The client does not escalate a single-link failure.** When a peer connection
   fails and another participant has already joined, the client tears down only
   that peer locally, keeps the call, and reports the link to the server. Pending
   invitations do not count: while the call is still 1:1, losing its only
   established peer keeps the existing whole-call failure behavior, even if a
   further target is ringing. A link that fails locally instead (a negotiation or
   WebRTC start error for one peer) takes the same path; a local audio failure
   affects every link and therefore still fails the call.
2. **The link is only declared dead after a relay-assisted retry.** Before
   reporting, the client force-refreshes the ICE config (stale TURN credentials are
   a prime suspect on long calls) and renegotiates the pair once with a forced TURN
   relay (re-offer by the lower client ID as usual), bounded by a 10 second
   per-peer establishment timer. TURN credentials are provisioned for every client,
   so a failed relay attempt means the pair genuinely cannot talk. The existing
   relay-reconnect machinery is reused for this.
3. **The server deterministically evicts the later joiner of the broken pair.** The
   protocol gains a link-scoped report, `CallErrorReason::PeerConnectionFailed`
   naming the unreachable peer. The server validates that reporter and named peer are
   both participants and records the report; it does not remove anyone on a single
   report. Once **both** endpoints of the pair have reported the link dead, the
   participant of the pair that joined the call later is removed from the call,
   reusing the regular leave semantics - including the leader rule: when the later
   joiner of the broken pair is the conference leader, the whole call ends, exactly
   as if the leader had left. Leadership never transfers, and a link eviction is no
   exception. The evicted client receives a distinct reason naming the peer it could
   not reach, so its UI can say "no connection to X" instead of a generic call end;
   the remaining participants receive a normal `CallUpdate`.
4. **A one-sided report waits; the reporter keeps retrying and re-reporting.** The
   server holds a half-reported link indefinitely - there is no timeout escalation.
   While the pair is in that limbo, the reporting client silently re-runs the
   relay-assisted retry every 30 seconds and re-files its report on every
   failed cycle. Each attempt either heals the link (transient asymmetry resolved;
   the standing half-report becomes moot and is pruned when either endpoint leaves)
   or drags the unsuspecting endpoint through a failed renegotiation, which is what
   its ICE stack needs to detect the failure and file the confirming report. The
   affected peer stays visible as disconnected in the UI throughout.

   The limbo is only worth keeping while some link still works. Once the last
   remaining link is declared dead as well, the client fails the call outright
   instead of lingering with no peer at all: every other participant is
   unreachable from it, and the eviction would sooner or later remove it anyway.

   Waiting indefinitely must not mean trusting indefinitely: a half-report only
   confirms while it is fresh. Reports expire after 90 seconds (three retry
   cycles); a confirming report that arrives later starts a new half-report
   instead of evicting. Without the expiry, a link that failed once, healed, and
   fails again much later could be evicted on a single fresh report paired with a
   leftover from the first incident. The reporter-side re-reporting keeps a
   genuinely dead link's report fresh, so the expiry never delays a legitimate
   eviction.

## Why (and what we rejected)

- **Status quo (eject the reporter, or the whole call):** turns one bad link into
  the loss of a participant with otherwise working links, or the loss of the whole
  conference when the reporter leads it. Rejected as the problem being solved.
- **Degraded conference (everyone stays, UI shows the broken link):** nobody gets
  kicked, but the conference stops being a shared room: the two affected controllers
  cannot hear each other and talk over one another. For ATC coordination a partial
  audio graph was judged worse than deterministically losing one participant.
- **Leader decides who leaves:** adds human latency in a situation that needs an
  immediate, consistent outcome on all clients. Rejected for the first iteration.
- **Evicting on a single report:** a one-sided report can be flakiness or abuse. Both
  endpoints observe an ICE failure of the same link, so requiring both reports is
  cheap and self-confirming. A client that cannot report (crashed, signaling gone)
  is removed by the existing disconnect cleanup instead.
- **Later joiner as the victim (rather than e.g. the higher client ID):** matches
  intuition ("the newcomer could not join properly"), preserves the established
  call, and is deterministic without negotiation. The established participants keep
  their working mesh; the newcomer with one dead link loses only its own seat.
- **Sparing the leader when it is the later joiner:** would keep the conference
  alive by evicting the earlier joiner instead, but makes the victim depend on
  leadership state and creates the only case where an established participant loses
  its seat to a newcomer's link. Rejected: determinism and the existing
  leader-leaves-ends-call rule win.
- **Timeout escalation for one-sided reports:** evicting after one report plus a
  timeout restores determinism but surrenders the self-confirming property - a
  single flaky or malicious client could then evict another by itself, just slower.
  Rejected in favor of the reporter-side retry loop, which converges to either a
  healed link or a legitimate second report in every realistic case.

## Consequences

- The protocol gains `CallErrorReason::PeerConnectionFailed(ClientId)`, used
  symmetrically: client to server it reports "my link to X is dead after the relay
  retry", server to the evictee it means "your link to X is dead, you are removed"
  (a `CallError` followed by `CallEnd`). Shipped with the 3.0.0 protocol break;
  the client shows the evictee "No connection to participant" on that peer's key.
- The server tracks a join sequence per participant and a per-call link-report
  table keyed by the normalized pair, pruned on every path that removes either
  endpoint and expired (90 s) on refresh rather than by a sweeper. Eviction reuses
  `end_call` and therefore inherits the ringing-guard serialization against
  concurrent accepts and invites. Server side this is
  `CallManager::report_link_failure` (`vacs-server/src/state/calls/manager.rs`) with
  `LINK_REPORT_TTL` in `state/calls.rs`.
- Client peer-failure paths split by call shape: a 1:1 call keeps the whole-call
  failure behavior (local teardown plus `WebrtcFailure` report), a conference
  reports per link and waits. During limbo the reporter keeps the peer in its
  roster as disconnected; it must not remove the participant locally - who leaves
  is the server's decision. Client side the shape test is `is_conference_link`
  (`vacs-client/src/app/state/calls.rs`) and the retry loop is `link_retry_task`
  with `LINK_RETRY_ESTABLISH_TIMEOUT` and `LINK_RETRY_INTERVAL`
  (`vacs-client/src/app/state/webrtc.rs`).
- Mesh health becomes leader-relevant: every new invitee multiplies the links that
  can be half-dead, so link state should be visible before growing the conference.
