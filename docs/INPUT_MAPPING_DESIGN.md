# Input Mapping Design

## Goals

Input mapping allows remapping and filtering of incoming control events to match platform or product requirements.

Primary goals:

- Deterministic key/button remapping
- Optional key blocking for policy/accessibility
- Minimal overhead in hot input path

## Core Types

- `InputMap`
  - key remap table
  - blocked key set
  - gamepad button remap table
- `MappedInjector<I>`
  - wraps any `InputInjector`
  - applies map before forwarding to underlying injector

## Processing Pipeline

1. Receive normalized input event.
2. If event is keyboard:
   - Drop if key is blocked.
   - Remap key if mapping exists.
3. If event is gamepad button:
   - Remap button if mapping exists.
4. Forward transformed event to wrapped injector.

Mouse motion/button/scroll paths pass through unchanged unless explicitly extended in future profiles.

## Design Properties

- Mapping is runtime-applied; no protocol changes required.
- Behavior is composable because `MappedInjector` wraps injector trait.
- Default map is pass-through (no remap, no block).

## Safety Controls

- Invalid mapping inputs are rejected at config parse boundary.
- Blocking/remap operations are explicit, not implicit.
- Mapping operations avoid panics in runtime event path.

## Testing Coverage

Current tests cover:

- Pass-through behavior with empty mapping
- Key remapping behavior
- Key blocking behavior
- Gamepad button remapping behavior

## Operational Guidance

- Keep profiles small and explicit.
- Version mapping profiles in source control.
- Validate profiles against target platform keycode expectations.

## Future Extensions

- Per-application/per-session profile switching
- Mouse and axis transformation profiles
- Import/export profile schema for desktop UI
