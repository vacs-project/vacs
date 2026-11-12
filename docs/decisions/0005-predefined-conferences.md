# 0005: Predefined conferences are a profile key that expands to a multi-target invite

- Status: accepted
- Date: 2026-10-10, amended 2026-11-10 (key behavior during a call, one ring per
  controller, 3.0 fallback, size and rate limits), accepted 2026-11-12 with the
  implementation (modify mode, key lit only when every station is covered, rate limit,
  wire signal for merged targets)
- Source: pre-planning session for #1266 (no recording). The key shape was proposed
  in that session; a maintainer confirmed it and settled the key behavior during a
  call and the 3.0 fallback in the review of 2026-11-10, and settled when the key shows
  call state and that a station is covered once any of its clients is in the call in
  the review of 2026-11-12.

## Context

#1266 asks for conference keys that start a call with a fixed set of stations. ADR
0002 rule 7 held fresh multi-target invites back from the UI, although the protocol
and server support them: `CallInvite.targets` is a set, and the server already tests
co-target listing (`multi_target_invitation_lists_the_co_targets` in
`vacs-server/tests/call.rs`). Under ADR 0002 rule 2, the caller becomes leader on the
second accept.

Three properties of today's server matter once such invites are common
(`handle_call_invite` in `vacs-server/src/ws/application_message.rs`):

- Each target is resolved to clients on its own. When one controller covers two of
  the listed stations, the second target fails `validate_call_attempt` with
  `AlreadyParticipant` and the caller's client shows an error on that key. Which of
  the two stations rings depends on `HashSet` iteration order. With top-down coverage
  this is the normal case for a conference key, not an edge case.
- The invite rate limiter charges one token per resolved target, with a burst of 8
  and one token back every 10 seconds. The burst doubles as the maximum target count
  of one invite.
- `max_conf_size` (default 8, a server setting) counts the caller, so one invite can
  bring in at most seven targets, and an invite over the limit is refused as a whole.

## Decision

1. The dataset adds an optional `station_ids` list to `DirectAccessKey` and
   `GeoPageButton`. `station_id` keeps its meaning and is not turned into a list. A
   key carries `station_id`, `station_ids` or `page`, never two of them.
2. Pressed while the client is in no call, such a key sends one fresh `CallInvite`
   with `targets = {Station(a), Station(b), ...}`. No new `CallTarget` variant is
   introduced.
3. Pressed while the client is in a call, the key invites its stations into that
   call, exactly as if each had been invited through the regular conference invite:
   only in conference modify mode, with the same `CallInvite` on the running call
   id, subject to ADR 0002 (only the leader may grow a conference, any participant
   may grow a leaderless call with nothing ringing). Stations already ringing or
   joined are left out, and the server leaves out stations whose controller is
   already in the call. Outside modify mode, or for a non-leader of a conference, a
   press does nothing. The key never ends, leaves or cancels a call: END does.
4. The key lights up only when every one of its stations is either ringing or in the
   call, whether or not the call was started from the key. If all of them are in the
   call, the key shows the in-call color. If at least one is still ringing, it shows
   the ringing color. With any station neither ringing nor in the call, the key shows
   no call state, so a 1:1 call with one of its stations does not light it, and
   neither does the subpage containing it. Rejected and errored stations show on
   their own station keys, the subpages containing them and the call display, never
   on the conference key.
5. ADR 0002 rule 7 is amended: multi-target invites become reachable from the UI
   through these keys only, fresh and into a running call.
6. Availability works like a regular station key. Stations covered by the caller are
   never rung. If at least one listed station not covered by the caller is online,
   the key is usable and the call rings every such online station. If none is
   online, the key shows as offline.
7. The client builds the target set from its station list (online, not `own`). The
   server keeps the authority: it resolves each target to clients, skips the
   caller's own client, rings what resolves, and answers the rest with a
   `TargetsNotFound` `CallError` while the call goes on.
8. One controller rings once. When several targets of one invite resolve to the same
   clients, the server rings those clients for one of them, chosen deterministically
   (the first in `CallTarget` order), and leaves the others out. On the wire each
   left-out target is answered with `CallError` `AlreadyParticipant`, the reason the
   server already used for a target whose controller is in the call. The client
   removes such a target without an error when it was invited together with other
   targets. A single-target invite keeps showing the error, as before. The client
   keeps no record of who covers what: `StationInfo` carries only `id` and `own`,
   and a local copy of the server's grouping would go stale when coverage changes
   during a call. The server applies the same grouping against the participants and
   ringing targets of a running call.
9. A 3.0 client shows such a key with its label and without a function. The key is
   not given a `station_id` as a fallback.

## Why (and what we rejected)

- **Turning `station_id` into a list:** a 3.0 client fails to parse the profile and
  loses `SessionInfo` entirely (ADR 0004).
- **A new `CallTarget::Conference(id)` variant:** a 3.0 recipient drops the invitation
  silently (ADR 0004).
- **A named call definition referenced by id:** considered while designing #460, then
  dropped once instant access became a per-key flag (ADR 0006).
- **`station_id` carrying a main target for 3.0 clients:** a key labeled as a
  conference would place a 1:1 call on old clients, and the dataset contract that a
  key has exactly one function would have to be relaxed. A dead key on a client that
  is about to update is the smaller surprise.
- **A separate in-call behavior for conference keys:** rejected by the maintainer. The
  key is a shortcut for inviting its stations, so it follows the rules a regular
  invite follows.
- **Showing call state on the key whenever any of its stations is in a call:** a 1:1
  with one listed station would light a conference key that did not place it. The
  key lights only when all of its stations are covered. Lighting it only for the call
  it placed was dropped: the client would have to carry the placing key's station
  list through the backend so a remote browser and the desktop agree, only to color
  one key, while the station keys already show the call.
- **Deduplicating targets on the client:** it does not know which controller covers a
  station, and adding that to `StationInfo` would only move the same grouping to
  every client.

## Consequences

- A 3.0 recipient gets a normal `CallInvitation` listing its co-targets. That
  receive path shipped untested and is now pinned by
  `vacs-client/e2e/specs/incoming-multi-target-call.e2e.ts` (ADR 0004 rule 4), which
  passes against the unchanged receive code: no gate for 3.0 recipients is needed.
  What a recipient sees is worth knowing when designing the caller side: the answer
  key reads "CONF" and does not name the caller, only the caller's blinking key shows
  who is calling, and co-targets appear nowhere until the recipient has accepted.
  The spec drives the current client through the raw protocol; an installed 3.0.x
  build against a newer caller stays a manual check.
- The on-disk dataset shape changes: `../vacs-data` docs, JSON schemas, importer and
  tools lock move with it. Ship order: server, then dataset.
- The dataset parser rejects a `station_ids` list longer than seven entries, the
  default `max_conf_size` minus the caller, so an oversized key fails validation and
  never loads. The limit is hardcoded because the parser cannot read the server
  setting; raising the server default means raising it in both places.
- The invite rate limiter keeps charging one token per resolved target, with the
  default burst raised from 8 to 16 so two presses of a seven-station key fit. One
  token per invite was considered and not chosen. The per-minute limit (40) is
  unchanged, and a deployment that overrides `call_invite` in its config needs the
  same raise.
- Point 8 changes `handle_call_invite` from resolving targets one by one to resolving
  all of them first and grouping by client set. The grouping also has to hold for
  point 3, against the participants and ringing targets of the running call; it
  does, through `CallManager::clients_in_call`.
- A station going offline between the key press and the server's resolution yields a
  partial `TargetsNotFound`. The client already keeps the call for the remaining
  targets (`remove_invited_targets` in the `CallError` handler of
  `vacs-client/src/app/state/signaling.rs`) and shows the error on that station's
  own key, never on the conference key (point 4).
- Callees see the call on the caller's station key, as for any call. A callee whose
  profile has the same conference key sees it light up only if every one of its
  stations is covered in the callee's call (point 4).
