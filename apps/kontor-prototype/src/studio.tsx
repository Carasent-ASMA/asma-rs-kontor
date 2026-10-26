import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { SCENARIOS } from "./scenarios";

/**
 * Review studio (plain DOM, not part of the plugin): one toolbar and the same
 * embed in a phone frame and a desktop frame. Each frame is its own fixture
 * host, so a frame reload resets its state. The URL names what is shown: every
 * change is a history entry, and Back and Forward restore it. A frame is keyed by
 * its address, so a change mounts a new frame: navigating the old one in place
 * would add a history entry of its own, and Back would then rewind only the frame.
 */
type Layout = "both" | "phone" | "desktop";
type StudioState = { readonly scenario: string; readonly theme: "dark" | "light"; readonly layout: Layout };

const PROVENANCE = "The plugin's own client source on react-native-web with a fixture host. Fictional data, except scenarios marked captured, which read an operator-local capture frozen at a recorded time; no Paseo connection. Navigation runs in the host shim; settings saves, links, the clipboard and toasts are mocked and logged in each frame's Host log. Browser rendering, not a device.";

function readState(search: string): StudioState {
  const params = new URLSearchParams(search);
  const layout = params.get("layout");
  return {
    scenario: SCENARIOS.find((value) => value.id === params.get("scenario"))?.id ?? SCENARIOS[0]!.id,
    theme: params.get("theme") === "light" ? "light" : "dark",
    layout: layout === "phone" || layout === "desktop" ? layout : "both",
  };
}
const queryOf = (state: StudioState) => `?scenario=${state.scenario}&theme=${state.theme}&layout=${state.layout}`;

const control = (active: boolean): CSSProperties => ({ font: "inherit", fontSize: 13, minHeight: 44, minWidth: 44, padding: "0 12px", borderRadius: 8, border: `1px solid ${active ? "#2E9E66" : "#3a3f3e"}`, background: active ? "#20744A" : "#1E2120", color: "#fafafa", cursor: "pointer" });
const panelStyle: CSSProperties = { position: "fixed", zIndex: 10, boxSizing: "border-box", background: "#181B1A", border: "1px solid #3a3f3e", borderRadius: 10, boxShadow: "0 8px 24px rgba(0,0,0,.45)", color: "#fafafa" };

/** A labelled group of toggle buttons; the pressed one is the current value. */
function Toggle<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: readonly { value: T; text: string }[]; onChange: (value: T) => void }) {
  return <div role="group" aria-label={label} style={{ display: "flex", gap: 4 }}>
    {options.map((option) => <button key={option.value} type="button" aria-pressed={value === option.value} style={control(value === option.value)} onClick={() => onChange(option.value)}>{option.text}</button>)}
  </div>;
}

/**
 * Closes on Escape, on a press outside, or on its own close button; focus returns to the opener.
 * The panel opens under its opener, kept inside the window.
 */
function usePopover(width: number) {
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<CSSProperties>({});
  const opener = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const show = () => {
    const anchor = opener.current!.getBoundingClientRect();
    const drawn = Math.min(width, window.innerWidth - 24);
    const top = anchor.bottom + 6;
    setPlace({ top, left: Math.max(12, Math.min(anchor.left, window.innerWidth - drawn - 12)), width: drawn, maxHeight: Math.max(160, window.innerHeight - top - 12) });
    setOpen(true);
  };
  const close = (focus: boolean) => { setOpen(false); if (focus) opener.current?.focus(); };
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!panel.current?.contains(event.target as Node) && !opener.current?.contains(event.target as Node)) close(false); };
    // On the document, so Escape works wherever focus is while the panel is open.
    const escape = (event: globalThis.KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); close(true); } };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", escape); };
  }, [open]);
  return { open, show, place, opener, panel, close };
}

function CloseButton({ onClose, autoFocus }: { onClose: () => void; autoFocus?: boolean }) {
  return <button type="button" aria-label="Close" autoFocus={autoFocus} onClick={onClose} style={{ ...control(false), padding: 0, border: "none", background: "transparent" }}>✕</button>;
}

/** Searchable scenario list. The highlighted scenario's description shows under the list. */
function ScenarioPicker({ value, onChange }: { value: string; onChange: (id: string) => void }) {
  const popover = usePopover(420);
  const [text, setText] = useState("");
  const [active, setActive] = useState(0);
  const current = SCENARIOS.find((scenario) => scenario.id === value)!;
  const needle = text.trim().toLocaleLowerCase();
  const matches = SCENARIOS.filter((scenario) => `${scenario.title} ${scenario.summary}`.toLocaleLowerCase().includes(needle));
  const highlighted = matches[Math.min(active, matches.length - 1)];
  const openList = () => { setText(""); setActive(Math.max(0, SCENARIOS.findIndex((scenario) => scenario.id === value))); popover.show(); };
  const choose = (id: string) => { popover.close(true); onChange(id); };
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, Math.min(matches.length - 1, Math.min(index, matches.length - 1) + (event.key === "ArrowDown" ? 1 : -1))));
    } else if (event.key === "Enter" && highlighted) {
      event.preventDefault();
      choose(highlighted.id);
    }
  };
  return <div style={{ minWidth: 0 }}>
    <button ref={popover.opener} type="button" aria-haspopup="dialog" aria-expanded={popover.open} onClick={() => popover.open ? popover.close(false) : openList()}
      style={{ ...control(popover.open), display: "flex", alignItems: "center", gap: 8, maxWidth: 340 }}>
      <span style={{ color: "#A1A5A4" }}>Scenario</span>
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontWeight: 600 }}>{current.title}</span>
      <span aria-hidden>▾</span>
    </button>
    {popover.open ? <div ref={popover.panel} role="dialog" aria-label="Choose a scenario" onKeyDown={onKeyDown}
      style={{ ...panelStyle, ...popover.place, padding: 8, display: "flex", flexDirection: "column", gap: 6 }}>
      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
        <input autoFocus value={text} onChange={(event) => { setText(event.target.value); setActive(0); }} placeholder="Search scenarios" aria-label="Search scenarios"
          role="combobox" aria-expanded aria-controls="studio-scenarios" aria-activedescendant={highlighted ? `studio-scenario-${highlighted.id}` : undefined} aria-describedby="studio-scenario-description"
          style={{ flex: 1, minWidth: 0, minHeight: 44, boxSizing: "border-box", padding: "0 10px", font: "inherit", fontSize: 14, color: "#fafafa", background: "#111312", border: "1px solid #3a3f3e", borderRadius: 8 }} />
        <CloseButton onClose={() => popover.close(true)} />
      </div>
      <ul id="studio-scenarios" role="listbox" aria-label="Scenarios" style={{ listStyle: "none", margin: 0, padding: 0, overflow: "auto", minHeight: 0 }}>
        {matches.map((scenario) => <li key={scenario.id} id={`studio-scenario-${scenario.id}`} role="option" aria-selected={scenario === highlighted}
          onPointerMove={() => setActive(matches.indexOf(scenario))} onClick={() => choose(scenario.id)}
          style={{ minHeight: 44, display: "flex", alignItems: "center", gap: 8, padding: "0 10px", borderRadius: 6, cursor: "pointer", background: scenario === highlighted ? "#272A29" : "transparent" }}>
          <span style={{ flex: 1 }}>{scenario.title}</span>
          {scenario.id === value ? <span style={{ color: "#2E9E66", fontSize: 12 }}>current</span> : null}
        </li>)}
        {matches.length === 0 ? <li role="presentation" style={{ padding: 10, color: "#A1A5A4" }}>No scenario matches “{text}”.</li> : null}
      </ul>
      <p id="studio-scenario-description" aria-live="polite" style={{ margin: 0, padding: "6px 10px", color: "#A1A5A4", fontSize: 12, borderTop: "1px solid #252B2A" }}>{highlighted?.summary ?? ""}</p>
    </div> : null}
  </div>;
}

/** The current scenario's description, the data provenance and the bare embed, on demand. */
function About({ summary, embed }: { summary: string; embed: string }) {
  const popover = usePopover(380);
  return <div>
    <button ref={popover.opener} type="button" aria-haspopup="dialog" aria-expanded={popover.open} onClick={() => popover.open ? popover.close(false) : popover.show()}
      style={{ ...control(popover.open), color: "#E8C27A" }}>Prototype · fixtures ⓘ</button>
    {popover.open ? <div ref={popover.panel} role="dialog" aria-label="About this preview"
      style={{ ...panelStyle, ...popover.place, overflow: "auto", padding: "4px 4px 4px 12px", fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center" }}><strong style={{ flex: 1 }}>About this preview</strong><CloseButton autoFocus onClose={() => popover.close(true)} /></div>
      <p style={{ margin: "0 8px 8px 0" }}>{summary}</p>
      <p style={{ margin: "0 8px 0 0", color: "#A1A5A4" }}>{PROVENANCE}</p>
      <a href={embed} target="_blank" rel="noreferrer" style={{ color: "#A1A5A4", fontSize: 12, minHeight: 44, display: "inline-flex", alignItems: "center" }}>Open embed in a tab ↗</a>
    </div> : null}
  </div>;
}

export function Studio() {
  const [state, setState] = useState(() => readState(window.location.search));
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    // The address names what is shown from the first frame on, including an unknown layout.
    history.replaceState(null, "", queryOf(state));
    const restore = () => setState(readState(window.location.search));
    window.addEventListener("popstate", restore);
    return () => window.removeEventListener("popstate", restore);
  }, []);
  const change = (patch: Partial<StudioState>) => {
    const next = { ...state, ...patch };
    if (queryOf(next) === queryOf(state)) return;
    history.pushState(null, "", queryOf(next));
    setState(next);
  };
  const { scenario, theme, layout } = state;
  const embed = (pointer: "coarse" | "fine") => `?embed=1&scenario=${scenario}&theme=${theme}&pointer=${pointer}&n=${nonce}`;
  const current = SCENARIOS.find((value) => value.id === scenario)!;
  return <div style={{ display: "flex", flexDirection: "column", height: "100%", color: "#fafafa", fontSize: 13 }}>
    <header style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center", padding: "8px 12px", borderBottom: "1px solid #252B2A", background: "#181B1A" }}>
      <h1 style={{ position: "absolute", width: 1, height: 1, overflow: "hidden", clip: "rect(0 0 0 0)" }}>Kontor plugin · review prototype</h1>
      <ScenarioPicker value={scenario} onChange={(id) => change({ scenario: id })} />
      <Toggle label="Device" value={layout} onChange={(value) => change({ layout: value })} options={[
        { value: "both", text: "Phone + desktop" }, { value: "phone", text: "Phone 390×844" }, { value: "desktop", text: "Desktop 1440×900" },
      ]} />
      <Toggle label="Theme" value={theme} onChange={(value) => change({ theme: value })} options={[{ value: "dark", text: "Paseo dark" }, { value: "light", text: "Paseo light" }]} />
      <button type="button" style={control(false)} onClick={() => setNonce(nonce + 1)}>Reset frames</button>
      <About summary={`${current.title}: ${current.summary}`} embed={embed("fine")} />
    </header>
    <div style={{ flex: 1, minHeight: 0, overflow: "auto", display: "flex", gap: 16, padding: 16, alignItems: "flex-start" }}>
      {layout !== "desktop" ? <figure data-device="phone" style={{ margin: 0 }}>
        <iframe key={embed("coarse")} title="Phone frame (touch pointer)" src={embed("coarse")} style={{ width: 390, height: 844, border: "1px solid #3a3f3e", borderRadius: 18, background: "#181B1A" }} />
        <figcaption style={{ color: "#A1A5A4", fontSize: 11, marginTop: 4 }}>Phone · 390×844 · touch pointer emulated · browser rendering, not a device</figcaption>
      </figure> : null}
      {layout !== "phone" ? <figure data-device="desktop" style={{ margin: 0, flex: layout === "both" ? 1 : undefined, minWidth: 0 }}>
        <div style={{ width: layout === "both" ? "100%" : 1440, height: 900, overflow: "hidden" }}>
          <iframe key={embed("fine")} title="Desktop frame (mouse pointer)" src={embed("fine")} style={{ width: 1440, height: 900, border: "1px solid #3a3f3e", borderRadius: 8, background: "#181B1A", transformOrigin: "0 0", transform: layout === "both" ? "scale(var(--desktop-scale, 0.62))" : undefined }} />
        </div>
        <figcaption style={{ color: "#A1A5A4", fontSize: 11, marginTop: 4 }}>Desktop · 1440×900{layout === "both" ? " shown scaled" : ""} · mouse pointer</figcaption>
      </figure> : null}
    </div>
  </div>;
}
