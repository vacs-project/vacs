# 0002: One conference leader, assigned when the call grows, never transferred

- Status: accepted
- Date: 2026-08-24 (leadership on the wire), amended 2026-08-31 (scope, self hang-up
  on non-droppable keys), 2026-09-02 (accept blocked while in a conference),
  implemented 2026-09-04 (protocol 3.0.0)
- Source: adhoc-conferences review sessions

## Context

Ad-hoc conferences let a participant of an established 1:1 call invite further
targets. Somebody has to be allowed to grow and shrink the call, and every client
has to agree on who that is: the client keys double as drop buttons in a conference,
so a wrong guess about permissions either lets a participant kick others or makes a
press do nothing the server accepts.

The first implementation guessed leadership locally (the frontend assumed the
inviter) and gated key presses on that guess, which disagreed with the server in both
directions and desynchronized the store after refused drops.

## Decision

1. **A 1:1 call has no leader.** Leadership only exists while a call is a
   conference, meaning three or more joined participants. A conference that shrinks
   back to two participants loses its leader again.
2. **The leader is the participant whose invite turned the call into a
   conference.** When the accept that makes the third participant join is
   processed, the server assigns leadership to that participant's inviter (the
   source of its invitation), not to the original caller of the 1:1 call. While no
   leader exists and nothing is ringing, any participant may invite; while an invite
   batch is still ringing, only the client that opened it may add targets.
3. **Only the leader may grow or shrink a conference.** Invites into a conference
   and drops of joined participants by anyone else are refused by the server. A
   still-ringing target may be cancelled by its inviter regardless of leadership,
   and a two-party call has nothing to drop from.
4. **Leadership never transfers.** When the leader leaves, disconnects, or is
   evicted (ADR 0001), the whole call ends for everyone. The leader ending the
   call therefore ends it for all participants; a non-leader leaving shrinks the
   call.
5. **Leadership is carried on the wire, not derived.** `CallInvitation` and
   `CallUpdate` carry `conference_leader`; clients gate their UI on that field. The
   inviter may assume leadership optimistically between sending the invite and the
   first update.
6. **Non-leader key presses on other participants hang up the presser.** A
   non-leader in a conference cannot drop anyone, so pressing another participant's
   key leaves the conference instead. Pressing a key that shows another
   participant's pending invitation does nothing.
7. **Scope: conferences grow only from an established call.** Every UI entry point
   invites a single target; a fresh multi-target invite is deliberately unreachable
   from the UI even though the protocol allows it. Incoming calls keep ringing while
   in a conference but cannot be accepted until the current call ends.

## Why (and what we rejected)

- **Growing inviter as leader, rather than the original caller:** the code marks
  the choice as deliberate (`accept_call`), and the sessions record no rationale
  beyond that.
- **Transferring leadership when the leader leaves:** rejected; the sessions record
  only that the leader leaving ends the call by design. ADR 0001 builds on the
  same rule for deterministic leader eviction.
- **A distinct protocol error for refused drops:** rejected as unreachable from a
  correctly gated UI. A refused drop keeps `NotConferenceLeader` and the server
  follows it with an authoritative `CallUpdate`, so a client with stale leadership
  state converges instead of desynchronizing.
- **Ignoring a non-leader's press on another participant's key:** the maintainers
  chose "leave the conference" over a dead key; no further rationale was recorded.
  The pending-invitation case is display-only because the presser cannot cancel an
  invite it did not send.
- **Fresh multi-target invites from the UI:** prepared in the protocol and server
  but held back. Preset conferences are a separate, later change; this record covers
  only ad-hoc growth from an established call.
- **Accepting an incoming call while in a conference:** rejected; the existing 1:1
  rule (hang up first) is kept. No further rationale was recorded.

## Consequences

- The server assigns leadership in `CallManager::accept_call` and enforces it in
  `attempt_call` (`StartCallError::NotConferenceLeader`) and `drop_target`
  (`DropTargetOutcome::NotPermitted`), all in
  `vacs-server/src/state/calls/manager.rs`; the leader-leaves branch of `end_call`
  and the disconnect cleanup end the call for everyone. The rules are pinned by the
  manager tests `leadership_goes_to_the_growing_inviter_not_the_original_caller`,
  `updates_carry_the_conference_leader_until_the_call_shrinks`,
  `dropping_a_participant_requires_the_conference_leader` and the wire tests in
  `vacs-server/tests/call.rs`.
- The protocol documents the authorization rules on
  `CallInvitation::conference_leader` (`vacs-protocol/src/ws/server/calls.rs`).
- The frontend derives `isConferenceLeader` from the wire and gates the CONF key
  (`components/ui/ConferenceButton.tsx`), the invite path (`startCall` in
  `stores/call-store.ts`) and the key press matrix
  (`hooks/station-key-interaction-hook.ts`, `components/ui/DirectAccessClientKey.tsx`)
  on it. The two key implementations mirror each other by hand and have to be kept
  in step.
- A leader crash costs everyone the call. Mesh health and leadership are therefore
  visible on the call display, and growing a conference is worth doing from the
  most stable participant.
- Supporting preset conferences or fresh multi-target calls later means revisiting
  rule 7 only; the leadership rules already cover a fresh multi-target call (the
  caller becomes leader on the second accept).
