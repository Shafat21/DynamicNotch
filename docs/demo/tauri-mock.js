// Stand-in for Tauri in the website demo (docs/demo): the real island frontend runs in an iframe and this
// feeds it sample data and events. Commands not listed resolve to null. Not loaded by the app itself.
(() => {
  const now = () => Math.floor(Date.now() / 1000);
  const handlers = new Map(); // event name -> [callback ids]
  const emit = (event, payload) => (handlers.get(event) || []).forEach((id) => window[`_${id}`]?.({ event, id, payload }));

  const art =
    "data:image/svg+xml;utf8," +
    encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><defs><linearGradient id="g" x1="0" y1="0" x2="0.6" y2="1"><stop offset="0" stop-color="#ff7a59"/><stop offset=".55" stop-color="#c2407a"/><stop offset="1" stop-color="#3a1f5c"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)"/><circle cx="66" cy="34" r="17" fill="#f7c873"/><path d="M0 70c18-14 30-12 44-2s30 8 56-10v42H0z" fill="#2b1846" opacity=".85"/></svg>`);

  const DUR = 243;
  let pos = 101;
  const hourly = [["3PM", "30°", "partly"], ["4PM", "29°", "sunny"], ["5PM", "28°", "cloudy"], ["6PM", "27°", "rainy"], ["7PM", "26°", "cloudy"]];
  const at = (min) => Date.now() + min * 60000;
  const sessions = [
    { id: "s1", agent: "claude", project: "webshop", cwd: "C:/code/webshop", status: "working", message: "Wiring Stripe checkout into the cart page", updatedAt: now() },
    { id: "s2", agent: "codex", project: "api-server", cwd: "C:/code/api-server", status: "done", message: "Added pagination to /orders", updatedAt: now() - 240 },
  ];
  const snapshot = {
    perf: { cpu: 23, ram: 61, disk: 48, down: 2.3, up: 0.3 },
    weather: { city: "Dhaka", temp: "29°", desc: "Partly Cloudy", icon: "partly", feels: "33°", high: "31°", low: "25°", humidity: 74, wind: 9, rain: 10, hourly: hourly.map(([time, temp, icon]) => ({ time, temp, icon })) },
    calendar: {
      configured: true, desktop: true, error: null, fatal: false, tomorrowFirst: null,
      today: [
        { id: "e1", title: "Ship v0.3", start: at(-600), end: at(600), allDay: true, join: null },
        { id: "e2", title: "Design review", start: at(24), end: at(54), allDay: false, join: "https://meet.example.com/design" },
        { id: "e3", title: "1:1 with Sam", start: at(120), end: at(150), allDay: false, join: null },
      ],
    },
    media: { state: "Playing", title: "Midnight City", artist: "M83", accent: "#c2407a", duration: DUR, seekable: true },
    "media-progress": { position: pos, duration: DUR },
    "media-art": { art },
    lyric: "Waiting in a car, waiting for a ride in the dark",
    bridge: { online: true, connected: true, agents: [{ id: "claude", label: "Claude Code", connected: true }, { id: "codex", label: "Codex", connected: true }] },
    sessions,
    usage: { fiveHour: { usedPercentage: 42, resetsAt: now() + 3 * 3600 }, sevenDay: { usedPercentage: 63, resetsAt: now() + 3 * 86400 }, sessionCostUsd: 1.84, updatedAt: now(), limitReached: false },
    display: { uiScale: 1, edge: "top", monitor: { index: 0, name: "Demo", width: 1920, height: 1080, scale: 1 } },
  };
  const tasks = [
    { id: 1, name: "Review PR #42", category: "Work", time: "", color: "#8fb0ff" },
    { id: 2, name: "Water the plants", category: "Home", time: "", color: "#4ade9a" },
  ];
  const settings = { location: "Dhaka", lat: 23.8, lon: 90.4, animationStyle: "Fluid Blobs", autostart: true, reminderSound: true };
  const notes = [{ id: 1, title: "Ideas", body: "Pal could wave when a build finishes", pinned: true, updatedAt: now() }];

  const REPLIES = {
    get_snapshot: snapshot, list_tasks: tasks, get_settings: settings, list_notes: notes, list_apps: [],
    list_reminders: [], due_reminders: [], list_clips: [], list_shelf: [], notetaker_settings: { enabled: false },
  };

  window.__TAURI_INTERNALS__ = {
    invoke(cmd, args) {
      if (cmd === "plugin:event|listen") {
        const list = handlers.get(args.event) || [];
        list.push(args.handler);
        handlers.set(args.event, list);
        return Promise.resolve(args.handler);
      }
      if (cmd === "set_hit_rect") hit = args?.rect || args || hit;
      if (cmd === "prompt_answer" || cmd === "answer_prompt") setTimeout(() => emit("prompt-closed", { id: 1 }), 150);
      return Promise.resolve(cmd in REPLIES ? structuredClone(REPLIES[cmd]) : null);
    },
    transformCallback(cb, once) {
      const id = Math.floor(Math.random() * 1e9);
      window[`_${id}`] = (p) => { if (once) delete window[`_${id}`]; cb(p); };
      return id;
    },
    unregisterCallback(id) { delete window[`_${id}`]; },
    convertFileSrc: (p) => p,
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

  // hover: the page reports its hit rect; inside it = hovering (like the Windows cursor poll)
  let hit = { x: 0, y: 0, w: 0, h: 0 };
  let hovering = false, userHover = false;
  const setHover = (on) => { if (on !== hovering) { hovering = on; emit("hover", on); } };
  addEventListener("pointermove", (e) => {
    const r = hit.w ? hit : { x: innerWidth / 2 - 120, y: 0, w: 240, h: 60 };
    const inside = e.clientX >= r.x - 8 && e.clientX <= r.x + r.w + 8 && e.clientY >= r.y - 8 && e.clientY <= r.y + r.h + 8;
    userHover = inside || (hovering && e.clientY < (r.y + r.h) * 1.1 + 300);
    setHover(userHover);
  });
  document.documentElement.addEventListener("mouseleave", () => { userHover = false; setHover(false); });

  // keep it alive: song progress, system stats, agent sessions
  setInterval(() => { pos = pos >= DUR ? 0 : pos + 1; emit("media-progress", { position: pos, duration: DUR }); }, 1000);
  const walk = (v, s, lo, hi) => Math.min(hi, Math.max(lo, v + (Math.random() * 2 - 1) * s));
  setInterval(() => {
    const p = snapshot.perf;
    p.cpu = walk(p.cpu, 9, 6, 48); p.ram = walk(p.ram, 0.8, 58, 65); p.down = walk(p.down, 1.2, 0.2, 6.5); p.up = walk(p.up, 0.12, 0.04, 0.7);
    emit("perf", { ...p, cpu: Math.round(p.cpu), ram: Math.round(p.ram) });
  }, 2000);

  // when nobody hovers: open the dashboard now and then, and let an agent finish its work
  let tick = 0;
  setInterval(() => {
    tick++;
    if (!userHover) setHover(tick % 3 !== 0);
    const s = sessions[0];
    s.status = tick % 4 === 3 ? "done" : "working";
    s.updatedAt = now();
    emit("sessions", structuredClone(sessions));
  }, 4000);
})();
