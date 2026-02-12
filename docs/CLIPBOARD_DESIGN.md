# Clipboard Synchronization Design

## Goals

Clipboard sync must make copy/paste between host and client feel immediate while staying safe under untrusted network input.

Primary goals:

- Bidirectional text clipboard synchronization
- Loop prevention so updates do not bounce indefinitely
- Bounded payload size and bounded polling overhead
- Clear failure behavior when clipboard backends are unavailable

## Non-Goals

- Binary clipboard (images/files) in v0.0.4
- Clipboard history synchronization
- Cross-session clipboard persistence

## Protocol Model

Clipboard updates are sent as `ControlMessage::Clipboard` with this payload:

- `text: String`

The receiving side applies the update to the local clipboard backend and updates local anti-echo state.

## Runtime Flow

Each side runs a polling loop (500ms):

1. Read local clipboard text.
2. Compare against `last_clipboard_text`.
3. If changed, send `ClipboardMessage` to peer.
4. On receive, apply text locally and update `last_clipboard_text`.

This guarantees local updates are propagated while remote-applied text does not get immediately re-emitted.

## Loop Prevention Strategy

Loop prevention is state-based, not timestamp-based:

- `last_clipboard_text` tracks the last applied/sent value.
- Incoming remote text sets this value immediately after `set_text`.
- Polling compares against this cached value and suppresses resend.

This avoids races caused by minor timestamp drift or backend-specific event ordering.

## Safety Controls

- Maximum text size enforced: `rift_core::MAX_CLIPBOARD_TEXT_BYTES`
- Oversized messages are ignored with warning logs.
- Clipboard text is treated as opaque text and not interpreted as commands.
- Control characters are passed through clipboard storage, but transport and parser limits still apply.

## Failure Behavior

- If clipboard backend initialization fails, session continues without clipboard sync.
- If `get_text` or `set_text` fails during runtime, error is logged and loop continues.
- Clipboard failures are non-fatal and do not tear down media/input session.

## Platform Notes

- Current runtime path uses `ArboardClipboard` in `wavry-platform`.
- Android paths may be gated depending on runtime support.
- Clipboard synchronization is best-effort, not guaranteed-delivery.

## Observability

Current logs include:

- Clipboard updates received from peer
- Oversized clipboard payload rejection
- Clipboard backend startup failures

Recommended next metric additions:

- Clipboard update send/receive counters
- Clipboard update reject counters by reason

## Security Considerations

- Clipboard content may contain secrets; do not log clipboard text content.
- Enforce strict size limits to prevent memory abuse.
- Do not execute clipboard text or use it as config input.

## Future Extensions

- Optional binary clipboard channel with explicit allow-list and size quotas
- Event-driven clipboard backends to reduce polling latency and CPU wakeups
- Per-session policy toggle: allow, receive-only, send-only, disabled
