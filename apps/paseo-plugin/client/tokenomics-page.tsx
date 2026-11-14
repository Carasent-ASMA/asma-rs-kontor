import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useRpc } from "@getpaseo/plugin/client";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { Linking, ScrollView, Text, View } from "react-native";
import { Action, Card, Copy, Disclosure, type Colors } from "./page-controls";
import { ACTIVITY_METRICS, DIMENSION_LABELS, FILTER_DIMENSIONS, TOKENOMICS_PAGES, exportTokenomics, readTokenomics, tokenomicsQuerySchema, type TokenomicsPage as Page, type TokenomicsQuery, type TokenomicsTable } from "../shared/tokenomics";

type Option = { value: string; label: string };
const choices = (values: readonly string[]): Option[] => values.map((value) => ({ value, label: value }));
function DateField({ label, value, colors, onDate }: { label: string; value: string | null; colors: Colors; onDate: (value: string | null) => void }) {
  const [draft, setDraft] = useState(value ?? ""), [error, setError] = useState(false);
  return <View><TextInput accessibilityLabel={`${label} date YYYY-MM-DD`} placeholder={`${label}: YYYY-MM-DD`} value={draft} onChangeText={setDraft} onBlur={() => {
    const valid = tokenomicsQuerySchema.shape.from.safeParse(draft || null);
    setError(!valid.success); if (valid.success) onDate(valid.data);
  }} style={{ color: colors.foreground, borderColor: colors.border, borderWidth: 1, padding: 9, minHeight: 44, minWidth: 170 }} />
  {error ? <Copy colors={colors} small>Enter a valid date as YYYY-MM-DD.</Copy> : null}</View>;
}
export function TokenomicsChoice({ label, options, value, onChange, colors, multiple = false }: {
  label: string; options: Option[]; value: string[]; onChange: (value: string[]) => void; colors: Colors; multiple?: boolean;
}) {
  const [open, setOpen] = useState(false), [search, setSearch] = useState(""), [offset, setOffset] = useState(0);
  const filtered = options.filter((o) => o.label.toLowerCase().includes(search.toLowerCase()));
  const summary = multiple ? `${value.length} selected` : options.find((o) => value.includes(o.value))?.label ?? "None";
  return <Disclosure label={`${label}: ${summary}`} colors={colors} open={open} onToggle={setOpen}>
    <TextInput accessibilityLabel={`Find ${label}`} placeholder={`Find ${label}`} value={search} onChangeText={(text) => { setSearch(text); setOffset(0); }} style={{ color: colors.foreground, backgroundColor: colors.surface0, borderColor: colors.border, borderWidth: 1, padding: 9, minHeight: 44 }} />
    {multiple ? <View style={{ flexDirection: "row", flexWrap: "wrap" }}><Action small colors={colors} label="All" onPress={() => onChange(options.map((o) => o.value))} /><Action small colors={colors} label="Clear" onPress={() => onChange([])} /></View> : null}
    {filtered.slice(offset, offset + 40).map((o) => <Action key={o.value} small colors={colors} label={o.label} selected={value.includes(o.value)} onPress={() => {
      onChange(multiple ? value.includes(o.value) ? value.filter((v) => v !== o.value) : [...value, o.value] : [o.value]); if (!multiple) setOpen(false);
    }} />)}
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center" }}>
      <Action small colors={colors} label="Previous options" disabled={!offset} onPress={() => setOffset(Math.max(0, offset - 40))} />
      <Copy colors={colors} small>{`${Math.min(offset + 1, filtered.length)}–${Math.min(offset + 40, filtered.length)} of ${filtered.length}`}</Copy>
      <Action small colors={colors} label="More options" disabled={offset + 40 >= filtered.length} onPress={() => setOffset(offset + 40)} />
    </View>
  </Disclosure>;
}

export function TokenomicsTableView({ table, colors, compact, query, change, enabled, onLink }: { table: TokenomicsTable; colors: Colors; compact: boolean; query: TokenomicsQuery; change: (patch: Partial<TokenomicsQuery>) => void; enabled: boolean; onLink: (url: string) => void }) {
  const offset = query.offsets[table.id] ?? query.offset;
  const numeric = table.id === "timeline" ? 5 : table.columns.findIndex((_, i) => table.rows.some((r) => typeof r.cells[i] === "number"));
  const max = Math.max(0, ...table.rows.map((r) => typeof r.cells[numeric] === "number" ? Math.abs(r.cells[numeric] as number) : 0));
  const format = (value: string | number | null | undefined) => value == null ? "—" : typeof value === "number" ? value.toLocaleString("en-US", { maximumFractionDigits: 3 }) : value;
  return <Card title={table.title} colors={colors}>
    {table.note ? <Copy colors={colors} muted small>{table.note}</Copy> : null}
    {table.chart ? <Copy colors={colors} muted small>Bars scale to the visible rows; every value is available below. Search or page to inspect the full series.</Copy> : null}
    <TokenomicsChoice label={`Sort ${table.title}`} options={choices(table.columns)} value={query.sort ? [query.sort] : []} colors={colors} onChange={(v) => change({ sort: v[0]!, descending: query.sort === v[0] ? !query.descending : true, offsets: {} })} />
    <Action small colors={colors} label={query.descending ? "Sort descending" : "Sort ascending"} onPress={() => change({ descending: !query.descending })} />
    {table.rows.length ? table.rows.map((r) => <View key={r.key} style={{ borderTopWidth: 1, borderColor: colors.border, paddingVertical: 8, gap: 4 }}>
      <Text selectable style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{r.cells.slice(0, Math.max(1, numeric)).map(format).join(" · ")}</Text>
      {r.hasChildren ? <Action small colors={colors} label={`${query.expanded.includes(r.key) ? "Collapse" : "Expand"} ${r.cells.slice(0, (r.depth ?? 0) + 1).map(format).join(" → ")}`} expanded={query.expanded.includes(r.key)} onPress={() => change({ expanded: query.expanded.includes(r.key) ? query.expanded.filter((key) => key !== r.key) : [...query.expanded, r.key] })} /> : null}
      {table.chart && max > 0 && typeof r.cells[numeric] === "number" ? <View accessibilityRole="image" accessibilityLabel={`${table.columns[numeric]}: ${format(r.cells[numeric])}`} style={{ height: 8, backgroundColor: colors.surface2, borderRadius: 4 }}><View style={{ height: 8, width: `${Math.max(0, Math.min(100, 100 * Math.abs(r.cells[numeric] as number) / max))}%`, backgroundColor: colors.accent, borderRadius: 4 }} /></View> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: compact ? 6 : 12 }}>
        {r.cells.map((c, i) => <View key={i} style={{ width: compact ? "47%" : 172, minWidth: 100, gap: 2 }}><Copy colors={colors} muted small>{table.columns[i]}</Copy><Copy colors={colors}>{format(c)}</Copy></View>)}
      </View>
      {r.detail.length ? <Disclosure label={`Details: ${format(r.cells[0])}`} colors={colors}>{r.detail.map((text, i) => <Copy key={i} colors={colors} small>{text}</Copy>)}</Disclosure> : null}
      {r.url ? <Action small colors={colors} label="Open source" onPress={() => onLink(r.url!)} /> : null}
    </View>) : <Copy colors={colors} muted>Nothing matches this page and selection. Missing sources are described above.</Copy>}
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      <Action small colors={colors} label={`Previous ${table.title}`} disabled={!enabled || offset === 0} onPress={() => change({ offsets: { ...query.offsets, [table.id]: Math.max(0, offset - 50) } })} />
      <Copy small colors={colors}>{`${Math.min(offset + 1, table.total)}–${Math.min(offset + 50, table.total)} of ${table.total}`}</Copy>
      <Action small colors={colors} label={`Next ${table.title}`} disabled={!enabled || offset + 50 >= table.total} onPress={() => change({ offsets: { ...query.offsets, [table.id]: offset + 50 } })} />
    </View>
  </Card>;
}

export function TokenomicsPage({ hostId, online, colors, compact, initialQuery, onQuery, onCopy, initialOffset = 0, onScroll }: {
  hostId: string; online: boolean; colors: Colors; compact: boolean; initialQuery?: TokenomicsQuery;
  onQuery: (q: TokenomicsQuery) => void; onCopy: (text: string) => Promise<void>; initialOffset?: number; onScroll: (offset: number) => void;
}) {
  const [query, setQuery] = useState(() => tokenomicsQuerySchema.parse(initialQuery ?? {}));
  const [exporting, setExporting] = useState(false), [exportNote, setExportNote] = useState<string | null>(null);
  const read = useRpc(readTokenomics), exportCsv = useRpc(exportTokenomics);
  const result = useQuery({ queryKey: ["tokenomics", hostId, query], queryFn: () => read(query), enabled: online, retry: false });
  const view = result.data, options = view?.options ?? {};
  const change = (patch: Partial<TokenomicsQuery>) => {
    const next = tokenomicsQuerySchema.parse({ ...query, offsets: {}, ...patch }); setQuery(next); onQuery(next); setExportNote(null);
  };
  const picker = (label: string, key: keyof TokenomicsQuery, items: Option[], selected: string) => <TokenomicsChoice key={label} label={label} colors={colors} options={items} value={[selected]} onChange={(v) => change({ [key]: (key === "rev" || key === "compare") && !v[0] ? null : v[0] })} />;
  const pageGroup = TOKENOMICS_PAGES[query.page][0], livePage = query.page.startsWith("cr-") || query.page.startsWith("dv-"), unfiltered = query.page === "progress" || query.page === "usage";
  const pageOptions = useMemo(() => Object.entries(TOKENOMICS_PAGES).filter(([, p]) => p[0] === pageGroup).map(([value, p]) => ({ value, label: p[1] })), [pageGroup]);
  const link = (url: string) => { if (/^https:\/\//.test(url)) void Linking.openURL(url).catch(() => setExportNote("This device could not open the source link.")); };
  return <ScrollView testID="tokenomics-page" contentOffset={{ x: 0, y: initialOffset }} onScroll={(event) => onScroll(event.nativeEvent.contentOffset.y)} scrollEventThrottle={100} style={{ flex: 1, backgroundColor: colors.surface0 }} contentContainerStyle={{ padding: compact ? 10 : 18, gap: 12 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: compact ? 20 : 24, fontWeight: "600" }}>Tokenomics</Text>
    <Copy colors={colors} muted>Read-only report on this host. Tokenomics filters are independent of the Kontor project and epic header.</Copy>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{["Tokenomics", "Code Review", "Delivery"].map((group) => <Action key={group} colors={colors} label={group} selected={group === pageGroup} onPress={() => change({ page: Object.entries(TOKENOMICS_PAGES).find(([, p]) => p[0] === group)![0] as Page, search: "", sort: "" })} />)}</View>
    <View accessibilityRole="tablist" style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{pageOptions.map((p) => <Action tab key={p.value} colors={colors} label={p.label} selected={query.page === p.value} onPress={() => change({ page: p.value as Page, search: "", sort: "" })} />)}</View>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
      <Action colors={colors} small label={result.isFetching ? "Refreshing…" : "Refresh report"} disabled={!online || result.isFetching} onPress={() => { void result.refetch(); }} />
      <Action colors={colors} small label={exporting ? "Preparing CSV…" : "Copy full CSV"} disabled={!online || !view || view.status === "unavailable" || exporting || result.isError || result.isFetching} onPress={() => {
        setExporting(true); setExportNote(null);
        void exportCsv(query).then(async (value) => { if (value.revision !== view?.revision) { setExportNote("Report changed since this view. Refresh before exporting."); return; } await onCopy(value.content); setExportNote(`${value.filename}: all matching rows copied as CSV, including rows beyond display pages.`); }).catch(() => setExportNote("CSV export or clipboard copy failed. Refresh the report and retry.")).finally(() => setExporting(false));
      }} />
    </View>
    {!online ? <Copy colors={colors}>Host offline. Any retained snapshot below is stale; refresh and exports are unavailable.</Copy> : result.isError ? <Copy colors={colors}>Report read failed. Any retained snapshot below is stale. Refresh to retry.</Copy> : result.isPending ? <Copy colors={colors}>Reading Tokenomics report…</Copy> : null}
    {view ? <Card title={`Source: ${!online || result.isError ? "stale" : view.status}`} colors={colors}>
      <Copy colors={colors} small>{`Generated ${view.generated ?? "unknown"} · read ${view.readAt} · content ${view.revision?.slice(0, 12) ?? "unavailable"}`}</Copy>
      {view.diagnostics.map((text, i) => <Copy key={i} colors={colors} small>{text}</Copy>)}
    </Card> : null}
    {exportNote ? <Copy colors={colors}>{exportNote}</Copy> : null}
    <Card title="Filters and view" colors={colors}>
      {!unfiltered ? picker("Period", "period", options.period ?? [], query.period) : <Copy colors={colors} muted>This page ignores Period and the other Tokenomics filters.</Copy>}
      {!unfiltered && query.period === "custom" ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>{(["from", "to"] as const).map((key) => <DateField key={`${key}-${query[key]}`} label={key} value={query[key]} colors={colors} onDate={(date) => change({ [key]: date })} />)}</View> : null}
      {!livePage && query.page !== "usage" ? <>
        {picker(query.page === "progress" ? "To check" : "At revision", "rev", [{ value: "", label: "Latest check" }, ...(options.revisions ?? [])], query.rev ?? "")}
        {picker(query.page === "progress" ? "From check" : "Compare revision", "compare", [{ value: "", label: query.page === "progress" ? "Previous check" : "No comparison" }, ...(options.revisions ?? [])], query.compare ?? "")}
      </> : null}
      {!livePage && !unfiltered ? <Disclosure label="Workstreams, users and dimensions" colors={colors}>
        {picker("Workstreams shown", "show", [{ value: "usage", label: "Open, with usage" }, { value: "idle", label: "Open, no usage" }, { value: "report", label: "With usage" }, { value: "all", label: "All registered" }], query.show)}
        <TokenomicsChoice multiple label="Workstreams" options={options.workstreams ?? []} value={query.workstreams ?? (options.workstreams ?? []).map((o) => o.value)} colors={colors} onChange={(value) => change({ workstreams: value })} />
        {FILTER_DIMENSIONS.map((key) => <TokenomicsChoice key={key} multiple label={DIMENSION_LABELS[key]!} options={options[key] ?? []} value={query.filters[key] ?? (options[key] ?? []).map((o) => o.value)} colors={colors} onChange={(value) => change({ filters: { ...query.filters, [key]: value } })} />)}
        <Action small colors={colors} label="Reset report filters" onPress={() => change({ filters: {}, workstreams: null, show: "usage", period: "all", from: null, to: null })} />
      </Disclosure> : null}
      {livePage ? <Copy colors={colors} muted>Live analytics use Period only. The revision, workstream and user selections do not apply.</Copy> : null}
      {query.page === "pivot" ? <>
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}><Action small colors={colors} label="Expand all groups" onPress={() => change({ expanded: (options.pivotNodes ?? []).map((o) => o.value) })} /><Action small colors={colors} label="Collapse all groups" onPress={() => change({ expanded: [] })} /></View>
        {query.groups.map((g, i) => <TokenomicsChoice key={i} label={`Group ${i + 1}`} options={options.groups ?? []} value={[g]} colors={colors} onChange={(value) => change({ groups: query.groups.map((v, n) => n === i ? value[0] as typeof g : v) })} />)}
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}><Action small colors={colors} label="Add group" disabled={query.groups.length >= 4} onPress={() => change({ groups: [...query.groups, "model"] })} /><Action small colors={colors} label="Remove last group" disabled={query.groups.length <= 1} onPress={() => change({ groups: query.groups.slice(0, -1) })} /><Action small colors={colors} label="Split cost by token type" selected={query.split} onPress={() => change({ split: !query.split })} /></View>
        <TokenomicsChoice label="Pivot preset" colors={colors} value={[]} options={choices(["ws,epic,task,seat", "ws,rg,seat", "model,ws", "vendor,model,tt", "seat,model", "acct,model", "day,ws", "task,rg"])} onChange={(v) => change({ groups: v[0]!.split(",") as TokenomicsQuery["groups"] })} />
      </> : null}
      {query.page === "roles" ? picker("Role share", "share", ["rg", "rc", "seat"].map((value) => ({ value, label: DIMENSION_LABELS[value]! })), query.share) : null}
      {query.page === "activity" ? <>
        {picker("Timeline metric", "metric", Object.entries(ACTIVITY_METRICS).map(([value, label]) => ({ value, label })), query.metric)}
        {picker("Timeline range", "range", choices(["all", "60", "30", "14"]), query.range)}
        {picker("Parallel work scope", "dual", options.dual ?? [], query.dual)}
        {picker("Daily measure", "measure", choices(["cost", "tok", "act"]), query.measure)}
        {picker("Daily stack", "stack", (options.groups ?? []).filter((o) => ["ws", "user", "tt", "vendor", "acct", "rg", "epic", "model"].includes(o.value)), query.stack)}
      </> : null}
      {query.page === "usage" ? <>
        {picker("All usage measure", "allMeasure", choices(["cost", "tok"]), query.allMeasure)}
        {picker("All usage stack", "allStack", [{ value: "c", label: "Category" }, { value: "g", label: "Workstreams / ad-hoc" }, { value: "s", label: "Sub-category" }, { value: "st", label: "Store / account" }, { value: "v", label: "Vendor" }], query.allStack)}
        {picker("All usage bars", "allGranularity", choices(["week", "day"]), query.allGranularity)}
        {picker("All usage from", "allFrom", [{ value: "first", label: "First recoverable data" }, { value: "aug", label: "Orchestrated era" }], query.allFrom)}
      </> : null}
    </Card>
    <TextInput accessibilityLabel="Search current Tokenomics page" placeholder="Search rows, people, PRs, issues or details" value={query.search} onChangeText={(search) => change({ search })} style={{ color: colors.foreground, backgroundColor: colors.surface1, borderWidth: 1, borderColor: colors.border, padding: 10, minHeight: 44 }} />
    {view?.notes.map((note, i) => <Copy key={i} colors={colors} muted small>{note}</Copy>)}
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 10 }}>{view?.tiles.map((tile) => <View key={tile.label} style={{ width: compact ? "47%" : 230 }}><Card title={tile.label} colors={colors}><Text selectable style={{ color: colors.foreground, fontSize: 20 }}>{tile.value ?? "—"}</Text>{tile.note ? <Copy colors={colors} muted small>{tile.note}</Copy> : null}</Card></View>)}</View>
    {view?.tables.map((table) => <TokenomicsTableView key={table.id} table={table} query={query} change={change} colors={colors} compact={compact} enabled={online && !result.isFetching} onLink={link} />)}
  </ScrollView>;
}
