# File Transfer Design

## Goals

File transfer in Wavry must remain secure, resumable, and non-disruptive to interactive media.

Primary goals:

- Safe file exchange between host and client
- Resume support across reordered/lost chunks
- Pause/resume/cancel/retry controls
- Fair-share bandwidth limiting so media remains responsive

## Non-Goals

- Multi-gigabyte archival transfer optimization
- Arbitrary directory sync
- End-user conflict resolution UI

## Protocol Primitives

RIFT control/media messages used:

- `FileHeader`
  - metadata: `file_id`, filename, size, checksum, chunking
- `FileChunk`
  - data plane payload for chunked file bytes
- `FileStatus`
  - receiver/peer control feedback: pending, in-progress, complete, error
  - command channel via message text (`pause`, `resume`, `cancel`, `retry`, `resume_chunk=`)

## Data Integrity Model

1. Sender computes SHA-256 checksum before transfer.
2. Receiver writes validated chunks into `.part` file.
3. On completion, receiver recomputes SHA-256 and compares against `FileHeader`.
4. Finalized file is moved atomically from `.part` to destination path.

Any checksum mismatch fails finalization.

## Resume and Control Semantics

### Resume

- Receiver reports `resume_chunk=<index>` in `FileStatus::InProgress`.
- Sender rewinds `next_chunk` if receiver indicates earlier missing chunk.

### Pause / Resume / Retry / Cancel

- `pause`: sender marks outgoing file paused.
- `resume`: sender unpauses.
- `retry`: sender restarts file from chunk 0 and re-sends header.
- `cancel`: sender removes outgoing file; receiver aborts partial state.

## Scheduler and Fairness (v0.0.4 Hardening)

Outgoing queues use round-robin scheduling over ready transfers.

Ready definition:

- Header not yet sent, or
- Header sent and file is not paused and not complete

Per tick behavior:

1. Rotate queue to next ready transfer.
2. Send header if needed.
3. Else send one chunk if token budget allows.
4. Rotate queue after progress to avoid single-file starvation.

This prevents paused/finished front entries from blocking all subsequent transfers.

## Congestion-Aware Budgeting

Transfer bandwidth uses a token bucket limiter driven by bitrate-derived budget.

Budget calculation:

- `budget = target_video_bitrate_kbps * share_percent`
- clamped between configured min and max kbps

Token bucket properties:

- Refill rate: budget kbps
- Burst capacity: up to 500ms of budget (with floor of 4 chunks)

Outcome:

- Media keeps priority
- Transfer can use spare envelope without unbounded burst

## Security Controls

- Filename sanitization (path traversal blocked)
- Offer validation (id, size, chunk shape, checksum format)
- Maximum file size enforcement
- Fixed-size chunk validation per index
- Status-message sanitization and size bound before parsing/logging

## Failure Handling

- Unknown `file_id` chunk => error status back to peer
- Invalid offer => reject and do not allocate incoming state
- Finalization error => error status and cleanup
- Missing-offer error => sender can restart with header

## Operator Controls

Server-side knobs:

- `--file-transfer-share-percent`
- `--file-transfer-min-kbps`
- `--file-transfer-max-kbps`
- `--file-max-bytes`
- `--send-file` (repeatable)
- `--file-out-dir`

Client-side transfer commands:

- `pause`, `resume`, `cancel`, `retry`

## Testing Requirements

Required test coverage areas:

- Outgoing seek/restart/pause/resume behavior
- Reordered chunk reconstruction
- Simulated loss + retransmit
- Resume-from-gap behavior
- Checksum mismatch rejection
- Queue scheduler selecting ready transfer over paused/finished entries

## Future Extensions

- Optional concurrent chunk windows per file (currently one chunk/tick)
- Explicit ack protocol field instead of command text parsing
- UI-level transfer prioritization (interactive vs bulk profile)
