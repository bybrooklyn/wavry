const logEl = document.getElementById("log");
const wsStatusEl = document.getElementById("wsStatus");

let ws = null;

function log(msg, obj) {
  const ts = new Date().toISOString();
  const suffix = obj === undefined ? "" : ` ${JSON.stringify(obj, null, 2)}`;
  logEl.textContent += `[${ts}] ${msg}${suffix}\n`;
  logEl.scrollTop = logEl.scrollHeight;
}

function gatewayBase() {
  return document.getElementById("gatewayBase").value.trim().replace(/\/+$/, "");
}

function token() {
  return document.getElementById("sessionToken").value.trim();
}

function targetUsername() {
  return document.getElementById("targetUsername").value.trim();
}

function wsUrl() {
  const base = gatewayBase();
  if (base.startsWith("https://")) {
    return `wss://${base.slice("https://".length)}/ws`;
  }
  if (base.startsWith("http://")) {
    return `ws://${base.slice("http://".length)}/ws`;
  }
  return `ws://${base}/ws`;
}

function setWsStatus(text, kind) {
  wsStatusEl.textContent = text;
  wsStatusEl.classList.remove("ok", "err");
  if (kind) wsStatusEl.classList.add(kind);
}

async function postJson(path, payload) {
  const res = await fetch(`${gatewayBase()}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const text = await res.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: res.status, body };
}

function sendBind() {
  const t = token();
  if (!t) {
    log("Cannot bind: missing session token");
    return;
  }
  const bindMsg = {
    type: "Bind",
    payload: { token: t },
  };
  ws.send(JSON.stringify(bindMsg));
  log("WS -> Bind", bindMsg);
}

document.getElementById("connectWsBtn").addEventListener("click", () => {
  if (ws && ws.readyState === WebSocket.OPEN) {
    log("WS already connected");
    return;
  }

  const url = wsUrl();
  ws = new WebSocket(url);
  setWsStatus(`WS: connecting ${url}`, null);
  log(`Connecting WS ${url}`);

  ws.onopen = () => {
    setWsStatus(`WS: connected ${url}`, "ok");
    log("WS open");
    sendBind();
  };
  ws.onclose = () => {
    setWsStatus("WS: disconnected", null);
    log("WS closed");
  };
  ws.onerror = () => {
    setWsStatus("WS: error", "err");
    log("WS error");
  };
  ws.onmessage = (evt) => {
    try {
      const parsed = JSON.parse(evt.data);
      log("WS <- message", parsed);
    } catch {
      log(`WS <- raw ${evt.data}`);
    }
  };
});

document.getElementById("disconnectWsBtn").addEventListener("click", () => {
  if (ws) {
    ws.close();
    ws = null;
  }
});

document.getElementById("offerBtn").addEventListener("click", async () => {
  const payload = {
    session_token: token(),
    target_username: targetUsername(),
    sdp: document.getElementById("sdpInput").value,
  };
  const out = await postJson("/webrtc/offer", payload);
  log("HTTP /webrtc/offer", out);
});

document.getElementById("answerBtn").addEventListener("click", async () => {
  const payload = {
    session_token: token(),
    target_username: targetUsername(),
    sdp: document.getElementById("sdpInput").value,
  };
  const out = await postJson("/webrtc/answer", payload);
  log("HTTP /webrtc/answer", out);
});

document.getElementById("candidateBtn").addEventListener("click", async () => {
  const payload = {
    session_token: token(),
    target_username: targetUsername(),
    candidate: document.getElementById("candidateInput").value,
  };
  const out = await postJson("/webrtc/candidate", payload);
  log("HTTP /webrtc/candidate", out);
});

log("Reference client loaded");
