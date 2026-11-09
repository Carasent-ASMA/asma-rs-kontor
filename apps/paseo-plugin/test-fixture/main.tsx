// R38 fixture entry: KontorScreen (the real root, with the compact bottom nav) over the
// kontor-prototype fixture runtime, so the Sessions selection bar, the wasted-bottom-space
// sources and the nav are all one DOM. Synthetic only: no Paseo host, every RPC answered in
// this page; effects stay in memory.
//
// Takeover limits of PRODUCTION length: the invoke() override answers
// sessions.navigator-capabilities with takeover limits exactly as the installed plugin's
// server answers them on a summaries-capable host (apps/paseo-plugin/server/
// session-navigator.ts readInstalledNavigatorCapabilities): [...TAKEOVER_LIMITS,
// GOVERNED_CAPABILITY_LIMIT]. TAKEOVER_LIMITS is imported verbatim from
// apps/paseo-plugin/shared/session-lifecycle.ts:96; GOVERNED_CAPABILITY_LIMIT is copied
// verbatim from apps/paseo-plugin/server/session-navigator.ts:15. The client joins these
// after TAKEOVER_EFFECTS (apps/paseo-plugin/client/session-takeover.tsx:18) into the
// disclosure the SelectionBar prints (client/session-operations.tsx:73-74).
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PaseoApiProvider, PluginRpcProvider } from "@getpaseo/plugin/client/host";
import { KontorScreen } from "@plugin/client/kontor-screen";
import { hostNavigation, navigate, sessionScopeKey } from "@plugin/client/navigation-state";
import { patchSessionView } from "@plugin/client/session-view-store";
import { TAKEOVER_LIMITS } from "@plugin/shared/session-lifecycle";
import { fakePaseo, HOST_ID, HOST_LABEL, invokeRpc as prototypeInvoke } from "@prototype/src/host/runtime";
import { shellColors } from "@prototype/src/host/theme";

// Verbatim from apps/paseo-plugin/server/session-navigator.ts (GOVERNED_CAPABILITY_LIMIT).
const GOVERNED_CAPABILITY_LIMIT = "A governed seat is handed to its existing authority writer; until that writer accepts the handoff summary before its successor's task effects, the plugin reports the exact missing governed input and retires nothing.";

const params = new URLSearchParams(window.location.search);
const compact = params.get("layout") !== "wide";
const colors = shellColors();
const queryClient = new QueryClient();
// R38-D2-r3 hold gate: the harness needs a content-neutral re-render (a refetch tick — the same
// synthetic data, so the bar's content size is unchanged) and calls this.
(window as unknown as { __r38Refetch: () => void }).__r38Refetch = () => { void queryClient.refetchQueries(); };

// One in-page record of every plugin RPC, like the bulk-archive fixture harness reads.
(window as unknown as { rpcCalls: { method: string; input: unknown }[] }).rpcCalls = [];
const calls = (window as unknown as { rpcCalls: { method: string; input: unknown }[] }).rpcCalls;

async function invoke(method: string, input: unknown): Promise<unknown> {
  calls.push({ method, input });
  // Production-length limits: the same list the installed plugin answers with on a real host.
  if (method === "sessions.navigator-capabilities") {
    const base = (await prototypeInvoke(method, input)) as { takeover: { supported: boolean; limits?: string[] } };
    return { ...base, takeover: { supported: true, limits: [...TAKEOVER_LIMITS, GOVERNED_CAPABILITY_LIMIT] } };
  }
  // ?archiveBusy=1: hold the archive preview in flight so the bar's Archive control refuses for a
  // REAL reason ("Archive waits for the host and any running archive."), the wide gate's deterministic
  // refusal. window.releaseArchive() resolves it.
  if (method === "sessions.lifecycle-preview" && params.get("archiveBusy") === "1") {
    await new Promise<void>((resolve) => { (window as unknown as { releaseArchive: () => void }).releaseArchive = resolve; });
  }
  return prototypeInvoke(method, input);
}

// Seed the plugin's in-memory route and view as a user would have left them: flat Sessions.
// state=mywork mirrors Igor's screenshot state as far as the fixture can: the My work scope and a
// status filter, so the layoutBar renders its appliedSummary line ("My work • Current", the same
// 22 pt line box as his "My work • Last activity • PR Review desk"). The fixture's rows carry no
// lastActivity (his "Last activity" status matches zero of them) and his PR Review desk shortcut
// would filter 24 sessions down to 3, so the closest renderable state stands in.
navigate(HOST_ID, { tab: "Sessions" });
const mywork = params.get("state") === "mywork";
patchSessionView(sessionScopeKey(HOST_ID, hostNavigation(HOST_ID).route), { query: "", status: mywork ? "current" : "all", work: mywork ? "mine" : "all", layout: "flat", toolbarOpen: true, advancedOpen: false });
const opened: unknown[] = [];
document.body.style.margin = "0";
document.body.style.background = colors.surface0;
document.body.style.fontFamily = "-apple-system, system-ui, sans-serif";
// The real host app hands the plugin a CONSTRAINED viewport: its window is fixed and the screen's
// flex chain shrinks inside it. #root stands in for that window: fixed height, display flex, so
// SafeAreaView's flex:1 actually fills it and the compact nav is pinned at the bottom.
const root = document.getElementById("root")!;
root.style.height = "100vh";
root.style.display = "flex";
root.style.overflow = "hidden";
createRoot(document.getElementById("root")!).render(
  <QueryClientProvider client={queryClient}>
    <PaseoApiProvider paseo={fakePaseo}>
      <PluginRpcProvider invoke={invoke}>
        <KontorScreen {...({ host: { id: HOST_ID, label: HOST_LABEL }, layout: { compact, platform: "web" as const }, theme: { colors },
          navigation: { openAgent: (input: unknown) => { opened.push(input); }, openWorkspace: () => undefined, openBrowser: () => undefined },
          // ?sheet=switch seeds the switch sheet open (kontor-screen.tsx:143), for the Limits-over-sheet pass.
          params: params.get("sheet") === "switch" ? { sheet: "switch" } : {} } as never)} />
      </PluginRpcProvider>
    </PaseoApiProvider>
  </QueryClientProvider>,
);

// Simulated device inset: desktop Chrome always reads env(safe-area-inset-bottom) as 0, so the
// under-nav measurement is driven from code instead. inflat=<px> sets that many pixels of
// padding-bottom on the SafeAreaView element (the only place the host inset enters this tree,
// apps/paseo-plugin/client/kontor-screen.tsx:354), as if the device reported it.
const inflat = Number(params.get("inflat") ?? "0");
if (inflat > 0) {
  const apply = () => {
    // React 19 renders the providers (DOM-free) directly under #root, so #root > div IS the
    // SafeAreaView element.
    const root = document.querySelector("#root > div") as HTMLElement | null;
    if (root) root.style.paddingBottom = `${inflat}px`;
  };
  // The SafeAreaView mounts with React's own inline style; re-apply after hydration settles.
  setTimeout(apply, 50);
  setTimeout(apply, 400);
}

// R15 (LSA disposition on the AUD r14 evidence): the fixture-only host-status hook for the
// committed host-offline gate. It lives ONLY here in test-fixture/ — never in the shipped client
// bundle; the bundle-identity check greps the built output for this name and requires an empty
// result. Neutral fixture name; the same runtime entry the prototype itself uses.
import { setHostStatus } from "@prototype/src/host/runtime";
(window as unknown as { __r38SetHostStatus: (next: "offline") => void }).__r38SetHostStatus = setHostStatus;
