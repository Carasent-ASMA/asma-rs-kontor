import { useEffect, useRef, useState } from "react";
import { View } from "react-native";
import { getPaseoClient } from "@getpaseo/plugin/client";
import { Action, type Colors } from "./page-controls";
import { SearchPicker } from "./search-picker";
import { isSourceId, readSource, readSourceCatalog, sourceRows, sourceState, type SourceChoice, type SourceRead, type SourceSession } from "./source-picker";

type Catalog = { hostId: string; entries: SourceSession[]; error: string | null; busy: boolean };
type Lookup = { query: string; hostId: string; value?: SourceRead | null; error?: string; busy?: boolean };

export function SourceSessionPicker({ hostId, colors, selected, onSelect, onBack }: {
  hostId: string; colors: Colors; selected: string | null; onSelect: (choice: SourceChoice) => void; onBack: () => void;
}) {
  const [query, setQuery] = useState(""), [includeArchived, setIncludeArchived] = useState(false), [revision, setRevision] = useState(0);
  const [catalog, setCatalog] = useState<Catalog>({ hostId, entries: [], error: null, busy: true });
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const currentHost = useRef(hostId), generation = useRef(0), mounted = useRef(true);
  currentHost.current = hostId;
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; generation.current++; }; }, []);
  useEffect(() => {
    const controller = new AbortController();
    setCatalog({ hostId, entries: [], error: null, busy: true });
    Promise.resolve().then(() => readSourceCatalog(getPaseoClient(hostId), controller.signal)).then(
      (entries) => { if (!controller.signal.aborted) setCatalog({ hostId, entries, error: null, busy: false }); },
      (error: unknown) => { if (!controller.signal.aborted) setCatalog({ hostId, entries: [], error: `Sessions could not be read: ${error instanceof Error ? error.message : String(error)}`, busy: false }); });
    return () => controller.abort();
  }, [hostId, revision]);
  const exact = isSourceId(query);
  useEffect(() => {
    if (!exact) return;
    const id = query.trim(), token = ++generation.current;
    setLookup({ hostId, query: id, busy: true });
    const timer = setTimeout(() => {
      Promise.resolve().then(() => readSource(getPaseoClient(hostId), id)).then(
        (value) => { if (mounted.current && token === generation.current && currentHost.current === hostId) setLookup({ hostId, query: id, value }); },
        (error: unknown) => { if (mounted.current && token === generation.current && currentHost.current === hostId) setLookup({ hostId, query: id, error: `Session lookup failed: ${error instanceof Error ? error.message : String(error)}` }); });
    }, 250);
    return () => { clearTimeout(timer); generation.current++; };
  }, [query, hostId, exact, revision]);
  const choose = (id: string) => {
    const token = ++generation.current;
    setLookup({ hostId, query: query.trim(), busy: true });
    Promise.resolve().then(() => readSource(getPaseoClient(hostId), id)).then((value) => {
      if (!mounted.current || token !== generation.current || currentHost.current !== hostId) return;
      setLookup({ hostId, query: query.trim(), value });
      if (value && !value.contextProblem) onSelect({ hostId, ...value });
    }, (error: unknown) => {
      if (mounted.current && token === generation.current && currentHost.current === hostId) setLookup({ hostId, query: query.trim(), error: `Session lookup failed: ${error instanceof Error ? error.message : String(error)}` });
    });
  };
  const resolved = lookup?.hostId === hostId && lookup.query === query.trim() ? lookup : null;
  const entries = catalog.hostId === hostId ? catalog.entries : [];
  const rows = exact ? resolved?.value ? [resolved.value.entry] : [] : sourceRows(entries, query, includeArchived);
  const busy = !!resolved?.busy || (exact ? !resolved : catalog.hostId !== hostId || catalog.busy);
  const error = resolved?.error ?? resolved?.value?.contextProblem ?? (!exact ? catalog.error : null);
  const message = resolved?.value === null ? "Session not found." : rows.length
    ? `${rows.length} ${query.trim() ? "matching sessions" : "newest sessions"}. Select one to check its context.` : "No sessions match this search.";
  return <SearchPicker name="kontor-source-picker" label="Choose a source session" placeholder="Search titles or paste a session ID" query={query}
    onQuery={(value) => { generation.current++; setLookup(null); setQuery(value); }} selected={selected} busy={busy} message={message} error={error}
    filters={<View style={{ flexDirection: "row" }}><Action label="Include archived" selected={includeArchived} colors={colors} onPress={() => { if (!exact) { generation.current++; setLookup(null); } setIncludeArchived(!includeArchived); }} /></View>}
    rows={rows.map((entry) => ({ id: entry.id, title: entry.title, detail: `${sourceState(entry)} · ${entry.id}`,
      problem: resolved?.value?.entry.id === entry.id ? resolved.value.contextProblem : null }))}
    onSelect={choose} onRetry={() => { setLookup(null); setRevision((value) => value + 1); }} onBack={onBack} colors={colors} />;
}
