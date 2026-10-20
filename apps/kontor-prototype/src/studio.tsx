import { useState } from "react";
import { SCENARIOS } from "./scenarios";

/**
 * Review studio (plain DOM, not part of the plugin): scenario list and the same
 * embed in a phone frame and a desktop frame. Each frame is its own fixture
 * host, so a frame reload resets its state.
 */
const params = new URLSearchParams(window.location.search);

export function Studio() {
  const [scenario, setScenario] = useState(params.get("scenario") ?? SCENARIOS[0]!.id);
  const [theme, setTheme] = useState(params.get("theme") === "light" ? "light" : "dark");
  const [layout, setLayout] = useState<"both" | "phone" | "desktop">((params.get("layout") as "both") ?? "both");
  const [nonce, setNonce] = useState(0);
  const embed = (pointer: "coarse" | "fine") => `?embed=1&scenario=${scenario}&theme=${theme}&pointer=${pointer}&n=${nonce}`;
  const current = SCENARIOS.find((value) => value.id === scenario) ?? SCENARIOS[0]!;
  const link = (id: string) => `?scenario=${id}&theme=${theme}&layout=${layout}`;
  const button = (active: boolean): React.CSSProperties => ({ font: "inherit", fontSize: 12, padding: "6px 10px", borderRadius: 6, border: "1px solid #3a3f3e", background: active ? "#20744A" : "#1E2120", color: "#fafafa", cursor: "pointer" });
  return <div style={{ display: "flex", height: "100%", color: "#fafafa", fontSize: 13 }}>
    <nav aria-label="Scenarios" style={{ width: 260, flexShrink: 0, borderRight: "1px solid #252B2A", padding: 12, overflow: "auto", background: "#181B1A" }}>
      <h1 style={{ fontSize: 14, margin: "0 0 4px" }}>Kontor plugin · review prototype</h1>
      <p style={{ color: "#A1A5A4", fontSize: 11, margin: "0 0 12px" }}>The plugin's own client source on react-native-web with a fixture host. Fictional data; no Paseo connection; host side effects are logged in each frame's Host log.</p>
      <ol style={{ listStyle: "none", padding: 0, margin: 0, display: "grid", gap: 4 }}>
        {SCENARIOS.map((value) => <li key={value.id}>
          <a href={link(value.id)} onClick={(event) => { event.preventDefault(); setScenario(value.id); history.replaceState(null, "", link(value.id)); }}
            aria-current={value.id === scenario ? "page" : undefined}
            style={{ display: "block", padding: "8px 10px", borderRadius: 6, textDecoration: "none", color: "#fafafa", background: value.id === scenario ? "#272A29" : "transparent", border: value.id === scenario ? "1px solid #20744A" : "1px solid transparent" }}>
            <div style={{ fontWeight: 600 }}>{value.title}</div>
            <div style={{ color: "#A1A5A4", fontSize: 11 }}>{value.summary}</div>
          </a>
        </li>)}
      </ol>
    </nav>
    <main style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", gap: 6, alignItems: "center", padding: 10, borderBottom: "1px solid #252B2A", flexWrap: "wrap" }}>
        <strong style={{ marginRight: 8 }}>{current.title}</strong>
        {(["both", "phone", "desktop"] as const).map((value) => <button key={value} type="button" style={button(layout === value)} aria-pressed={layout === value} onClick={() => setLayout(value)}>{value === "both" ? "Phone + desktop" : value === "phone" ? "Phone 390×844" : "Desktop 1440×900"}</button>)}
        <span style={{ width: 12 }} />
        {(["dark", "light"] as const).map((value) => <button key={value} type="button" style={button(theme === value)} aria-pressed={theme === value} onClick={() => setTheme(value)}>{value === "dark" ? "Paseo dark" : "Paseo light"}</button>)}
        <button type="button" style={button(false)} onClick={() => setNonce(nonce + 1)}>Reset frames</button>
        <a href={embed("fine")} target="_blank" rel="noreferrer" style={{ color: "#A1A5A4", fontSize: 12, marginLeft: "auto" }}>Open embed in a tab ↗</a>
      </div>
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", display: "flex", gap: 16, padding: 16, alignItems: "flex-start" }}>
        {layout !== "desktop" ? <figure style={{ margin: 0 }}>
          <iframe title="Phone frame (touch pointer)" src={embed("coarse")} style={{ width: 390, height: 844, border: "1px solid #3a3f3e", borderRadius: 18, background: "#181B1A" }} />
          <figcaption style={{ color: "#A1A5A4", fontSize: 11, marginTop: 4 }}>Phone · 390×844 · touch pointer emulated · browser rendering, not a device</figcaption>
        </figure> : null}
        {layout !== "phone" ? <figure style={{ margin: 0, flex: layout === "both" ? 1 : undefined, minWidth: 0 }}>
          <div style={{ width: layout === "both" ? "100%" : 1440, height: 900, overflow: "hidden" }}>
            <iframe title="Desktop frame (mouse pointer)" src={embed("fine")} style={{ width: 1440, height: 900, border: "1px solid #3a3f3e", borderRadius: 8, background: "#181B1A", transformOrigin: "0 0", transform: layout === "both" ? "scale(var(--desktop-scale, 0.62))" : undefined }} />
          </div>
          <figcaption style={{ color: "#A1A5A4", fontSize: 11, marginTop: 4 }}>Desktop · 1440×900{layout === "both" ? " shown scaled" : ""} · mouse pointer</figcaption>
        </figure> : null}
      </div>
    </main>
  </div>;
}
