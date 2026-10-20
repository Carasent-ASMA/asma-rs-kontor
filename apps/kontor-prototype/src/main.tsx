/**
 * Entry. `?embed=1` renders one fixture host with the plugin; without it the
 * studio frames phone and desktop embeds side by side.
 *
 * `pointer=coarse` stands in for a touch device: the plugin reads
 * `(pointer: coarse)` once when its bundle loads, so the override is installed
 * before the plugin modules are imported.
 */
const params = new URLSearchParams(window.location.search);
if (params.get("pointer") === "coarse" || params.get("pointer") === "fine") {
  const coarse = params.get("pointer") === "coarse";
  const original = window.matchMedia.bind(window);
  window.matchMedia = (query: string) => query.replace(/\s/g, "") === "(pointer:coarse)"
    ? ({ ...original(query), matches: coarse, media: query } as MediaQueryList)
    : original(query);
}

async function boot() {
  const { createRoot } = await import("react-dom/client");
  const { createElement } = await import("react");
  const root = createRoot(document.getElementById("root")!);
  if (params.get("embed") === "1") {
    const { HostShell } = await import("./host/shell");
    root.render(createElement(HostShell));
  } else {
    const { Studio } = await import("./studio");
    root.render(createElement(Studio));
  }
}
void boot();
