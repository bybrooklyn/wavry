# Android Modern UX Guidelines (Wavry)

This checklist aligns Wavry Android with current Android guidance for modern apps.

## Core product UX

- Prefer clear, task-oriented entry points:
  session start/stop, account sign-in/up, and status at a glance.
- Make primary actions obvious and fast:
  one-tap connect, one-tap retry/reconnect, and clear failure recovery.
- Keep status actionable:
  show connection mode (cloud/direct), signaling state, and concrete next step.

## Architecture and UI state

- Keep UI state in `ViewModel` and expose immutable state streams to Compose.
- Treat Compose screens as renderers of state + events.
- Normalize and validate user input before network and native calls.

## Adaptive layouts

- Use adaptive layouts so tablet/foldable widths do not waste space.
- Prefer side-by-side panes on large widths; use tabbed/stacked layout on phones.
- Ensure status chips and controls wrap cleanly on narrow screens.
- Base high-level layout decisions on window size classes (compact/medium/expanded).
- Keep activities resizable and avoid orientation locks for large-screen compatibility.

## Material 3 and visual system

- Use Material 3 tokens consistently for type, color, and elevation.
- Enable dynamic color on Android 12+ by default.
- Keep contrast and hierarchy strong for long sessions and low-light use.
- Use Material navigation patterns by window size:
  navigation bar (compact), navigation rail (medium/expanded), and navigation drawer when needed.

## Reliability and trust

- Surface auth/session failures with plain-language, user-actionable messages.
- Expose health/metrics endpoints for cloud auth and signaling diagnostics.
- Add smoke tests for register/login/logout and metrics availability.

## Performance and quality

- Track startup and interaction performance in CI and release builds.
- Add baseline profiles and macrobenchmarks for startup/connect flows.
- Validate UX quality against Android app quality checklists before release.

## Primary sources

- [Android app architecture recommendations](https://developer.android.com/topic/architecture)
- [Guide to app architecture](https://developer.android.com/topic/architecture/intro)
- [Compose state and state hoisting](https://developer.android.com/develop/ui/compose/state)
- [Adaptive layouts (Jetpack Compose)](https://developer.android.com/develop/ui/compose/layouts/adaptive)
- [Adaptive do's and don'ts](https://developer.android.com/develop/ui/compose/layouts/adaptive/adaptive-dos-and-donts)
- [Window size classes](https://developer.android.com/develop/ui/compose/layouts/adaptive/use-window-size-classes)
- [Material 3 for Android](https://developer.android.com/develop/ui/compose/designsystems/material3)
- [Material 3 navigation bar](https://developer.android.com/develop/ui/compose/components/navigation-bar)
- [Material 3 navigation rail](https://developer.android.com/develop/ui/compose/components/navigation-rail)
- [Material 3 navigation drawer](https://developer.android.com/develop/ui/compose/components/drawer)
- [Material 3 top app bar](https://developer.android.com/develop/ui/compose/components/app-bars)
- [Core app quality guidelines](https://developer.android.com/docs/quality-guidelines/core-app-quality)
