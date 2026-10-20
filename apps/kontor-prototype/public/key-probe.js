/*
 * Kontor key probe (ASMA-8541): does a Cmd shortcut reach this page, and with
 * which code, key and modifiers? The prototype's key-probe scenario loads this
 * file, and the same text can be pasted into a console, such as Paseo's
 * View > Toggle Developer Tools, to ask the same question of that page.
 *
 * It records key events with Cmd held, and the Cmd key itself; plain typing is
 * never recorded. At most 16 entries are kept in this page's memory and shown
 * in a box. Nothing is saved or sent anywhere. It never calls preventDefault or
 * stopPropagation and never moves focus. Recording stops after two minutes or
 * on Stop, and Close (or a reload) removes the box and its entries.
 */
(() => {
  "use strict";
  const LIMIT = 16;
  const DURATION_MS = 120000;
  globalThis.__kontorKeyProbe?.close();

  const started = Date.now();
  const entries = [];
  const byEvent = new WeakMap();
  let recording = true;

  const box = document.createElement("div");
  box.dataset.kontorKeyProbe = "";
  box.setAttribute("role", "region");
  box.setAttribute("aria-label", "Kontor key probe");
  Object.assign(box.style, {
    position: "fixed", right: "12px", bottom: "12px", zIndex: "2147483647", boxSizing: "border-box",
    width: "min(400px, calc(100vw - 24px))", maxHeight: "60vh", overflow: "auto", padding: "10px",
    background: "#181B1A", color: "#fafafa", border: "1px solid #E8C27A", borderRadius: "10px",
    font: "12px/1.45 ui-monospace, Menlo, monospace", boxShadow: "0 8px 24px rgba(0,0,0,.45)",
  });
  const line = (text, style) => {
    const element = document.createElement("div");
    element.textContent = text;
    Object.assign(element.style, style ?? {});
    box.append(element);
    return element;
  };
  line("Kontor key probe · Cmd shortcuts only, in memory", { fontWeight: "700", color: "#E8C27A" });
  // The buttons come first, so they stay in reach on a short screen.
  const controls = document.createElement("div");
  Object.assign(controls.style, { display: "flex", gap: "8px", margin: "6px 0" });
  box.append(controls);
  const where = line(surface());
  const layout = line("Layout: not available in this engine", { color: "#A1A5A4" });
  const focus = line("");
  const state = line("");
  const list = document.createElement("ol");
  Object.assign(list.style, { margin: "6px 0", paddingLeft: "22px" });
  box.append(list);
  const empty = line("No Cmd key event yet. Press Cmd with the key to test, once.", { color: "#A1A5A4" });
  const button = (label, onClick) => {
    const element = document.createElement("button");
    element.type = "button";
    element.textContent = label;
    Object.assign(element.style, { minWidth: "44px", minHeight: "44px", padding: "0 12px", font: "inherit", color: "#fafafa", background: "#1E2120", border: "1px solid #3a3f3e", borderRadius: "8px", cursor: "pointer" });
    element.addEventListener("click", onClick);
    controls.append(element);
    return element;
  };
  const copy = button("Copy", () => {
    const text = report();
    navigator.clipboard?.writeText(text).then(() => { copy.textContent = "Copied"; }, () => { copy.textContent = "Copy failed: select the text"; });
  });
  const stopButton = button("Stop", () => stop());
  button("Close", () => close());

  /** Where this page runs. The Paseo desktop renderer has the `paseoDesktop` bridge; a Paseo browser tab does not. */
  function surface() {
    const agent = navigator.userAgent;
    const electron = /Electron\/([\d.]+)/.exec(agent);
    const chromium = /Chrome\/([\d.]+)/.exec(agent);
    const safari = /Version\/([\d.]+).*Safari/.exec(agent);
    const firefox = /Firefox\/([\d.]+)/.exec(agent);
    const engine = electron ? `Electron ${electron[1]}${chromium ? `, Chromium ${chromium[1]}` : ""}`
      : chromium ? `Chromium ${chromium[1]}` : safari ? `Safari ${safari[1]}` : firefox ? `Firefox ${firefox[1]}` : "unknown engine";
    const paseo = "paseoDesktop" in globalThis ? "Paseo desktop renderer" : electron ? "Electron page (not the Paseo renderer)" : "browser page";
    return `${paseo} · ${engine} · ${location.protocol}//${location.host}`;
  }

  // The layout's character for each key position, where the engine offers it (Chromium only).
  navigator.keyboard?.getLayoutMap?.().then((map) => {
    const show = (code) => `${code} → ${map.has(code) ? JSON.stringify(map.get(code)) : "none"}`;
    layout.textContent = `Layout: ${["Backquote", "IntlBackslash", "Comma"].map(show).join(" · ")}`;
  }, () => { layout.textContent = "Layout: map refused by the engine"; });

  const modifiers = (event) => [event.metaKey && "Cmd", event.ctrlKey && "Ctrl", event.altKey && "Opt", event.shiftKey && "Shift", event.getModifierState?.("AltGraph") && "AltGr"].filter(Boolean).join("+") || "none";
  const yes = (value) => (value ? "yes" : "no");
  const describe = (entry) => entry.type === "focus lost" || entry.type === "focus back" ? `${entry.type} at=${entry.at}ms` : [
    `${entry.type} code=${entry.code || "(empty)"} key=${JSON.stringify(entry.key)} keyCode=${entry.keyCode} mods=${entry.modifiers}`,
    `repeat=${yes(entry.repeat)} composing=${yes(entry.composing)} trusted=${yes(entry.trusted)} prevented=${entry.prevented} stopped on the way=${entry.stopped === null ? "pending" : yes(entry.stopped)} at=${entry.at}ms`,
  ].join("\n   ");
  function report() {
    return [where.textContent, layout.textContent, ...entries.map((entry, index) => `#${index + 1} ${describe(entry)}`)].join("\n");
  }

  function draw() {
    list.replaceChildren(...entries.map((entry) => {
      const item = document.createElement("li");
      item.textContent = describe(entry);
      Object.assign(item.style, { whiteSpace: "pre-wrap", marginBottom: "4px" });
      return item;
    }));
    empty.style.display = entries.length ? "none" : "";
    focus.textContent = document.hasFocus() ? "This page has keyboard focus." : "This page does not have keyboard focus: click it first.";
    const left = Math.max(0, Math.ceil((started + DURATION_MS - Date.now()) / 1000));
    state.textContent = recording ? `Recording · ${entries.length}/${LIMIT} · stops in ${left} s` : `Stopped · ${entries.length} recorded`;
    stopButton.disabled = !recording;
  }

  const record = (entry) => {
    entries.push(entry);
    // Stop once the dispatch in hand has finished, so its bubble listener still runs.
    if (entries.length >= LIMIT) setTimeout(stop, 0);
    draw();
  };

  // Window capture listeners run in the order they were added, so the page's own run before this one:
  // "before the probe" means one of them took the key.
  function capture(event) {
    if (!recording || entries.length >= LIMIT || !(event.metaKey || event.key === "Meta")) return;
    const entry = {
      type: event.type === "keydown" ? "down" : "up", code: event.code, key: event.key, keyCode: event.keyCode, modifiers: modifiers(event),
      repeat: event.repeat, composing: event.isComposing || event.keyCode === 229, trusted: event.isTrusted,
      prevented: event.defaultPrevented ? "before the probe" : "no", stopped: null, reachedEnd: false, at: Date.now() - started,
    };
    byEvent.set(event, entry);
    record(entry);
    // After the dispatch: an event that never reached the window's bubble listener was stopped by a handler on its way.
    // (A microtask would run between listeners of a trusted event, before the bubble listener.)
    setTimeout(() => {
      entry.stopped = !entry.reachedEnd;
      if (entry.prevented === "no" && event.defaultPrevented) entry.prevented = "after the probe";
      draw();
    }, 0);
  }
  function bubble(event) {
    const entry = byEvent.get(event);
    if (entry) entry.reachedEnd = true;
  }
  // A window switch by the system shows as lost focus right after the Cmd press.
  function focusChange(event) {
    if (recording && entries.length < LIMIT) record({ type: event.type === "blur" ? "focus lost" : "focus back", at: Date.now() - started });
    else draw();
  }

  const listeners = [["keydown", capture, true], ["keyup", capture, true], ["keydown", bubble, false], ["keyup", bubble, false], ["focus", focusChange, false], ["blur", focusChange, false]];
  for (const [type, listener, phase] of listeners) window.addEventListener(type, listener, phase);
  const tick = setInterval(draw, 1000);
  const timeout = setTimeout(() => stop(), DURATION_MS);

  function stop() {
    if (!recording) return;
    recording = false;
    for (const [type, listener, phase] of listeners) window.removeEventListener(type, listener, phase);
    clearInterval(tick);
    clearTimeout(timeout);
    draw();
  }
  function close() {
    stop();
    entries.length = 0;
    box.remove();
    if (globalThis.__kontorKeyProbe === probe) delete globalThis.__kontorKeyProbe;
  }
  const probe = { stop, close, read: () => entries.map((entry) => ({ ...entry })), recording: () => recording };
  globalThis.__kontorKeyProbe = probe;
  document.body.append(box);
  draw();
})();
