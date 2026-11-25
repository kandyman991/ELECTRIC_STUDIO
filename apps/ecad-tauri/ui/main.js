// Electric Studio (Tauri shell). Forwards input to the Rust `event` command and paints the
// Frame it returns. No editing logic lives here: see crates/ecad-editor (AGENTS.md rule 6).
"use strict";

// Never fail silently: any script error or failed call is shown in the status bar.
const showError = (msg) => { const s = document.getElementById("status"); if (s) s.textContent = "Error: " + msg; };
window.addEventListener("error", (e) => showError(e.message));
window.addEventListener("unhandledrejection", (e) => showError(e.reason));
if (!window.__TAURI__) throw new Error("the Tauri API is missing (withGlobalTauri)");

const { invoke } = window.__TAURI__.core;
const appWindow = window.__TAURI__.window.getCurrentWindow();
// If the first frame hasn't arrived after 5 s, say so instead of showing an empty canvas.
setTimeout(() => { if (!frame) showError("no answer from the editor after 5 s"); }, 5000);

const ACCENT = "#409cff", TARGET = "#ffaa28", START = "#50dc78", QUIET = "#8c96a5";
const canvas = document.getElementById("canvas");
const ctx = canvas.getContext("2d");
const $ = (id) => document.getElementById(id);

let base = [];
let known = null;
let frame = null;
let queue = [];
let busy = false;
let paletteBuilt = false;

// ---- talking to Rust: events are queued and sent in batches, one request at a time ----

function send(ev) {
  const last = queue[queue.length - 1];
  if (ev.type === "pointerMove" && last && last.type === "pointerMove") queue[queue.length - 1] = ev;
  else queue.push(ev);
  flush();
}

async function flush() {
  if (busy) return;
  busy = true;
  try {
    do {
      const events = queue;
      queue = [];
      const f = await invoke("event", { events, known });
      if (f.base) { base = f.base; known = f.revision; }
      frame = f;
    } while (queue.length);
    render();
  } catch (e) {
    showError(e);
  } finally {
    busy = false;
  }
}

// ---- input ----

const pos = (e) => { const r = canvas.getBoundingClientRect(); return { x: e.clientX - r.left, y: e.clientY - r.top }; };
const button = (e) => (e.button === 1 ? "middle" : e.button === 0 ? "primary" : null);

canvas.addEventListener("pointermove", (e) => send({ type: "pointerMove", ...pos(e) }));
canvas.addEventListener("pointerdown", (e) => {
  const b = button(e);
  if (!b) return;
  e.preventDefault();
  canvas.setPointerCapture(e.pointerId);
  send({ type: "pointerMove", ...pos(e) });
  send({ type: "pointerDown", ...pos(e), button: b });
});
canvas.addEventListener("pointerup", (e) => { const b = button(e); if (b) send({ type: "pointerUp", ...pos(e), button: b }); });
canvas.addEventListener("wheel", (e) => { e.preventDefault(); send({ type: "wheel", ...pos(e), delta: -e.deltaY / 100 }); }, { passive: false });
canvas.addEventListener("auxclick", (e) => e.preventDefault());

window.addEventListener("keydown", (e) => {
  if (document.activeElement && document.activeElement.tagName === "INPUT") {
    if (e.key === "Enter") rename();
    return;
  }
  const cmd = e.ctrlKey || e.metaKey;
  const key = (k) => { e.preventDefault(); send({ type: "key", key: k }); };
  if (e.key === "Escape") key("escape");
  else if (e.key === "Delete" || e.key === "Backspace") key("delete");
  else if (cmd && e.key.toLowerCase() === "z") key(e.shiftKey ? "redo" : "undo");
  else if (cmd && e.key.toLowerCase() === "y") key("redo");
  else if (cmd && e.key.toLowerCase() === "s") { e.preventDefault(); send({ type: "save" }); }
  else if (!cmd && e.key.toLowerCase() === "r") key("rotate");
  else if (!cmd && e.key.toLowerCase() === "f") send({ type: "fit" });
  else if (!cmd && e.key.toLowerCase() === "w") send({ type: "tool", tool: "wire" });
});

document.querySelectorAll("[data-tool]").forEach((b) => b.addEventListener("click", () => send({ type: "tool", tool: b.dataset.tool })));
$("undo").addEventListener("click", () => send({ type: "key", key: "undo" }));
$("redo").addEventListener("click", () => send({ type: "key", key: "redo" }));
$("fit").addEventListener("click", () => send({ type: "fit" }));
$("save").addEventListener("click", () => send({ type: "save" }));
$("rename").addEventListener("click", rename);
function rename() { send({ type: "rename", tag: $("tag").value }); $("tag").blur(); }

function resize() {
  const r = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(r.width * dpr);
  canvas.height = Math.round(r.height * dpr);
  send({ type: "resize", width: r.width, height: r.height });
}
new ResizeObserver(resize).observe(canvas);

// ---- painting ----

function render() {
  if (!frame) return;
  const v = frame.view;
  const dpr = window.devicePixelRatio || 1;
  const sx = (p) => v.width / 2 + (p.x - v.center.x) * v.scale;
  const sy = (p) => v.height / 2 - (p.y - v.center.y) * v.scale;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, v.width, v.height);
  ctx.lineJoin = "round";

  const path = (pts) => { ctx.beginPath(); pts.forEach((p, i) => (i ? ctx.lineTo(sx(p), sy(p)) : ctx.moveTo(sx(p), sy(p)))); };
  const rgb = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

  ctx.lineWidth = 1.2;
  for (const prim of base) {
    const pts = prim.shape.points;
    if (prim.shape.kind === "polyline") {
      path(pts); ctx.strokeStyle = rgb(prim.rgb); ctx.stroke();
    } else {
      ctx.beginPath();
      for (let i = 0; i + 2 < pts.length; i += 3) {
        ctx.moveTo(sx(pts[i]), sy(pts[i])); ctx.lineTo(sx(pts[i + 1]), sy(pts[i + 1])); ctx.lineTo(sx(pts[i + 2]), sy(pts[i + 2])); ctx.closePath();
      }
      ctx.fillStyle = rgb(prim.rgb); ctx.fill();
    }
  }

  for (const o of frame.overlays) {
    if (o.type === "marker") {
      ctx.beginPath(); ctx.arc(sx(o.at), sy(o.at), o.kind === "open" ? 3.5 : 5, 0, 2 * Math.PI);
      if (o.kind === "open") { ctx.lineWidth = 1; ctx.strokeStyle = QUIET; ctx.stroke(); }
      else { ctx.fillStyle = o.kind === "target" ? TARGET : START; ctx.fill(); }
    } else if (o.type === "outline") {
      const x0 = sx(o.min) - 4, y0 = sy(o.max) - 4, x1 = sx(o.max) + 4, y1 = sy(o.min) + 4;
      ctx.lineWidth = o.selected ? 1.5 : 1; ctx.strokeStyle = o.selected ? ACCENT : QUIET;
      ctx.beginPath(); ctx.roundRect(x0, y0, x1 - x0, y1 - y0, 3); ctx.stroke();
    } else if (o.type === "wire") {
      path(o.points); ctx.lineWidth = 3; ctx.strokeStyle = o.selected ? ACCENT : QUIET; ctx.stroke();
    } else if (o.type === "rubberBand") {
      path(o.points); ctx.lineWidth = 1.5; ctx.strokeStyle = TARGET; ctx.stroke();
    } else if (o.type === "ghost") {
      ctx.lineWidth = 1; ctx.strokeStyle = QUIET; ctx.beginPath();
      for (const [a, b] of o.lines) { ctx.moveTo(sx(a), sy(a)); ctx.lineTo(sx(b), sy(b)); }
      ctx.stroke();
    }
  }
  chrome();
}

function chrome() {
  if (!paletteBuilt && frame.palette.length) {
    for (const item of frame.palette) {
      const b = document.createElement("button");
      b.textContent = item.description; b.title = "Place " + item.id; b.dataset.tool = item.id;
      b.addEventListener("click", () => send({ type: "tool", tool: item.id }));
      $("palette").appendChild(b);
    }
    paletteBuilt = true;
  }
  document.querySelectorAll("[data-tool]").forEach((b) => b.classList.toggle("active", b.dataset.tool === frame.tool));
  $("undo").disabled = !frame.undo; $("undo").title = frame.undo ? "Undo " + frame.undo : "Nothing to undo";
  $("redo").disabled = !frame.redo; $("redo").title = frame.redo ? "Redo " + frame.redo : "Nothing to redo";
  $("save").disabled = !frame.dirty;
  $("status").textContent = frame.status || "Ready";
  const tag = frame.selectedTag;
  $("selected").hidden = !tag; $("nothing").hidden = !!tag;
  if (tag && document.activeElement !== $("tag") && $("tag").dataset.for !== tag) { $("tag").value = tag; $("tag").dataset.for = tag; }
  const title = frame.title + " - Electric Studio (Tauri)";
  if (document.title !== title) { document.title = title; appWindow.setTitle(title).catch(() => {}); }
}
