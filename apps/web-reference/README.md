# Wavry Browser Reference Client Stub

This is a minimal browser-side reference stub for:

- WebSocket signaling bind to `/ws`
- HTTP WebRTC signaling posts to:
  - `/webrtc/offer`
  - `/webrtc/answer`
  - `/webrtc/candidate`

It does not implement full media playback. It is intended as a quick integration harness.

## Run locally

From this folder:

```bash
python3 -m http.server 8081
```

Then open:

- `http://localhost:8081`

Set Gateway Base URL (default `http://localhost:3000`) and paste a valid session token from `/auth/login`.
