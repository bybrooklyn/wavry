# Audio Routing Design

## Goals

Audio routing must allow operators to choose which audio source is streamed while preserving low-latency session behavior.

Primary goals:

- Deterministic route selection from CLI/runtime config
- Safe fallback behavior where route is unsupported
- Cross-platform defaults that do not silently break sessions

## Route Model

Configured route:

- `system` (default)
- `microphone`
- `app:<name>`
- `disabled`

Runtime enum:

- `AudioRouteSource::SystemMix`
- `AudioRouteSource::Microphone`
- `AudioRouteSource::Application(String)`
- `AudioRouteSource::Disabled`

## Current Platform Behavior

### macOS

- `system`: ScreenCaptureKit system mix capture
- `microphone`: CPAL input capture path
- `app:<name>`: ScreenCaptureKit application-filtered capture

### Linux

- `system`: PipeWire portal/system mix capture (PulseAudio fallback when needed)
- `microphone`: default microphone capture via GStreamer source path (`pulsesrc`/`autoaudiosrc`)
- `app:<name>`: best-effort PulseAudio app sink matching (`pactl` sink-input resolution), with fallback to `system`

### Windows

- `system`: WASAPI loopback capture on default render endpoint
- `microphone`: WASAPI capture on default input endpoint
- `app:<name>`: WASAPI process-loopback capture (`ActivateAudioInterfaceAsync`) targeting the selected process ID/name, with fallback to `system` on init failure

## Stream Path

1. Route is parsed from CLI/config.
2. Platform capturer initializes according to route.
3. Audio frames are encoded and forwarded as `MediaMessage::Audio` packets.
4. Client decodes and renders synchronized audio output.

## Error and Fallback Behavior

- `disabled` route returns an explicit startup error and disables audio stream.
- Unsupported routes emit warnings and fall back to `system` where implemented.
- Capturer runtime errors are logged; loop retries on transient capture errors.

## Safety and Security Controls

- No secret material or auth data is included in audio metadata.
- Audio route requests are validated and normalized at startup.
- Unsupported routes do not crash session startup.
- Capture failures do not crash whole process; stream continues without audio when needed.

## Configuration Surface

- CLI: `--audio-source`
- Env: `WAVRY_AUDIO_SOURCE`

Recommended operator behavior:

- Use `system` for broad compatibility.
- Use `microphone` only on platforms with verified support.
- Monitor warnings for fallback events in production logs.

## Future Work

- Linux per-application route selection via PipeWire node targeting
- Policy control: allowlist of application route targets
- Route capability probing exposed to UI and CLI diagnostics
