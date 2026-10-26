import { useEffect, useRef, useState } from "react";
import { Platform, Pressable, ScrollView, Text, View } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { createChoiceProblem, featureDefault, inheritedModeProblem, keptFeatureValues, readCreateFeatures, type CreateFeature, type CreateProvider, type FeatureValues } from "../shared/session-create";
import { Action, COARSE_POINTER, NO_PRESS_DELAY, pressFeedback, spaceActivates, type Colors } from "./page-controls";
import { readableInk } from "./session-status-ink";
import { modelAlias, modelVendor, routeLabel, type Vendor } from "./session-usage";
import { VendorMark, vendorName, vendorShortName } from "./vendor-mark";
import { SheetFooter } from "./operation-sheet";
import type { LeadModel } from "./new-lead-defaults";

/** The gaps the list draws: between rows, between providers. */
const ROW_GAP = 2, GROUP_GAP = 8;
/** Whole model rows listed at first, and added by each press of more. */
export const MODEL_BATCH = 6;

/** One model with its settings, as Use hands it back. `featureValues` holds only the listed settings the operator set; `modeId` is null when permissions are inherited. */
export type PickedModel = { readonly provider: string; readonly model: string; readonly effort: string | null; readonly modeId: string | null; readonly featureValues: FeatureValues };
export type ModelSettings = { readonly features: readonly CreateFeature[]; readonly problem: string | null };

/** The settings a desk offers for one model, read live; null while reading. A failed read is a problem, never an empty list. */
export function useCreateFeatures(projectId: string | null, provider: string | null, model: string | null): ModelSettings | null {
  const read = useRpc(readCreateFeatures);
  const key = projectId && provider && model ? JSON.stringify([projectId, provider, model]) : null;
  const [state, setState] = useState<(ModelSettings & { key: string }) | null>(null);
  useEffect(() => {
    if (!key) return;
    let live = true;
    read({ projectId: projectId!, provider: provider!, model: model! }).then((result) => { if (live) setState({ key, ...result }); },
      (error: unknown) => { if (live) setState({ key, features: [], problem: error instanceof Error ? error.message : String(error) }); });
    return () => { live = false; };
  }, [key]);
  return key && state?.key === key ? state : null;
}

/** A listed setting's value for a lead: the operator's, else the setting's own. */
export const featureValue = (feature: CreateFeature, values: FeatureValues) => values[feature.id] ?? featureDefault(feature);

/** "Normal" for each select and "Plan off" for each toggle, as the lead will run. */
export function featureLabels(features: readonly CreateFeature[], values: FeatureValues): { selects: string[]; toggles: string[] } {
  return {
    selects: features.flatMap((feature) => feature.type === "select" ? feature.options.filter((option) => option.id === featureValue(feature, values)).map((option) => option.label) : []),
    toggles: features.flatMap((feature) => feature.type === "toggle" ? [`${feature.label} ${featureValue(feature, values) === true ? "on" : "off"}`] : []),
  };
}

/**
 * Model & settings: the chosen model and its effort levels first, then the vendors whose models this host's available
 * routes offer, the browsed vendor's models under those routes, and the permissions and settings the host lists for
 * the chosen model. The list starts with a short batch of whole rows and more adds the next batch below them; a search
 * matches every row, shown or not. The picker scrolls as one page: its own scroller only where the surface around it
 * does not scroll already. Choosing another model keeps the effort and settings it also offers and says which it drops. Only a
 * caller that may change the new-lead default passes `defaults`; without it the choice is this operation's only. Make
 * default saves on Use; Reset clears the saved default at once and starts from the Codex default's own settings. A
 * caller that passes `permissions` shows them read-only and never emits a mode.
 */
export function ModelPicker({ colors, compact, providers, projectId, value, codex, onUse, onCancel, defaults, permissions, subtitle = "For new leads", onAuto, action, note, hostScrolls = false }: {
  colors: Colors; compact: boolean; providers: readonly CreateProvider[];
  /** The desk whose checkout scopes the settings read. */
  projectId: string | null;
  value: PickedModel | null; codex: LeadModel | null;
  onUse: (choice: PickedModel) => void; onCancel: () => void;
  defaults?: { readonly name: string; readonly make: (choice: PickedModel) => void; readonly reset: () => void };
  /**
   * Permissions a takeover inherits: one seat's own mode id on its own provider (null when nobody reported it), or
   * `seats` when each of several seats keeps its own. Either way they are shown read-only and no mode is emitted; a
   * model on a provider that cannot keep one seat's mode is refused here, as its host would.
   */
  permissions?: { readonly inherited: string | null; readonly provider: string } | { readonly seats: number };
  subtitle?: string;
  /** Offers Auto, each seat's own chain, above the models; it is checked while no model is picked, and Use then calls this. */
  onAuto?: () => void;
  /** The primary's label for the picked model's alias, or null for Auto. */
  action?: (alias: string | null) => string;
  /** What the choice does and does not change, above the footer. */
  note?: string;
  /** The surface around the picker scrolls already (the composer's popover or sheet), so the picker adds no scroller. */
  hostScrolls?: boolean;
}) {
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState<PickedModel | null>(value);
  // The choice the picker opened with leads the list (board B2); later picks leave the order alone, so rows never jump.
  const [opened] = useState(value);
  const leads = (provider: string, model?: string) => provider === opened?.provider && (model === undefined || model === opened.model) ? 0 : 1;
  const [makeDefault, setMakeDefault] = useState(false);
  // The vendor whose models are listed; browsing changes neither the choice nor its settings.
  const [browsing, setBrowsing] = useState<Vendor | null>(null);
  // How many rows the list shows, in list order; another vendor or search starts again from the first batch.
  const [revealed, setRevealed] = useState(MODEL_BATCH);
  // Permissions stay one line until opened, so Make default and Reset stay in reach.
  const [open, setOpen] = useState<"permissions" | null>(null);
  // What the last change of model could not keep, said until the next change.
  const [changed, setChanged] = useState<string | null>(null);
  // Every setting read so far, so a value the new model drops is still named as the operator set it.
  const seen = useRef(new Map<string, CreateFeature>());
  const toggle = (key: "permissions") => setOpen(open === key ? null : key);
  const provider = providers.find((entry) => entry.id === pick?.provider && entry.available) ?? null;
  const model = provider?.models.find((entry) => entry.id === pick?.model) ?? null;
  const settings = useCreateFeatures(projectId, provider?.id ?? null, model?.id ?? null);
  const tall = compact || COARSE_POINTER;
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 } as const;
  for (const feature of settings?.features ?? []) seen.current.set(feature.id, feature);
  // Re-choosing the checked model changes nothing. Another model keeps the effort it also offers, and every setting
  // until its own list is read.
  const choose = (next: CreateProvider, option: CreateProvider["models"][number]) => {
    if (pick?.provider === next.id && pick.model === option.id) return;
    const kept = pick?.effort && option.efforts.some((entry) => entry.id === pick.effort) ? pick.effort : null;
    const effort = kept ?? option.defaultEffort ?? option.efforts[0]?.id ?? null, was = model?.efforts.find((entry) => entry.id === pick?.effort);
    setChanged(pick?.effort && !kept ? `${option.label} does not offer effort ${was?.label ?? pick.effort}; it uses ${option.efforts.find((entry) => entry.id === effort)?.label ?? "no effort setting"}.` : null);
    setPick({ provider: next.id, model: option.id, effort, modeId: permissions ? null : pick?.provider === next.id ? pick.modeId : next.defaultMode ?? next.modes[0]?.id ?? null, featureValues: pick?.featureValues ?? {} });
  };
  const patch = (next: Partial<PickedModel>) => { if (pick) setPick({ ...pick, ...next }); };
  const tokens = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const available = providers.filter((entry) => entry.available).sort((a, b) => leads(a.id) - leads(b.id));
  const ordered = (entry: CreateProvider) => [...entry.models].sort((a, b) => leads(entry.id, a.id) - leads(entry.id, b.id));
  // Vendors in the list's order, so the opened choice's comes first, each with how many models the available routes offer.
  const vendors = new Map<Vendor, number>();
  for (const entry of available) for (const option of ordered(entry)) vendors.set(modelVendor(option.id), (vendors.get(modelVendor(option.id)) ?? 0) + 1);
  const shownVendor = browsing !== null && vendors.has(browsing) ? browsing : [...vendors.keys()][0] ?? null;
  // The list holds the browsed vendor's models under the routes that offer them; a search narrows within that vendor.
  const groups = available.map((entry) => ({ provider: entry, route: routeLabel(entry.id, entry.label),
    rows: ordered(entry).map((option) => ({ model: option, alias: modelAlias(option.id), isCodex: codex?.provider === entry.id && codex.model === option.id }))
      .filter((row) => modelVendor(row.model.id) === shownVendor
        && tokens.every((token) => `${row.model.label} ${row.model.id} ${row.alias} ${routeLabel(entry.id, entry.label)} ${entry.label}`.toLocaleLowerCase().includes(token))) }))
    .filter((group) => group.rows.length);
  const rowHeight = tall ? 52 : 44;
  let budget = revealed;
  const shown = groups.map((group) => { const rows = group.rows.slice(0, budget); budget -= rows.length; return { ...group, rows }; }).filter((group) => group.rows.length);
  const hidden = Math.max(0, groups.reduce((sum, group) => sum + group.rows.length, 0) - revealed);
  // The settings this model lists keep their values; the rest are dropped, and said. Values wait for the list to be read.
  const listed = settings ? settings.problem ? [] : settings.features : null;
  const values = listed && pick ? keptFeatureValues(listed, pick.featureValues, [...seen.current.values()]) : null;
  // Inherited permissions are the seat's own: one seat's mode is checked against the chosen provider; model and effort are checked here.
  const problem = onAuto && !pick ? null : !pick ? "Choose a model."
    : (permissions ? ("inherited" in permissions ? inheritedModeProblem(providers, permissions.provider, permissions.inherited, pick.provider) : null)
      ?? createChoiceProblem(providers.map((entry) => ({ ...entry, modes: [] })), { ...pick, modeId: null }) : createChoiceProblem(providers, pick))
      ?? (!values && Object.keys(pick.featureValues).length ? "Reading this model's settings…" : null);
  const effort = model?.efforts.find((entry) => entry.id === pick?.effort) ?? null;
  const modeId = permissions ? "inherited" in permissions ? permissions.inherited : null : pick?.modeId;
  // An inherited mode is named by the seat's own provider, never by a provider that happens to list the same id.
  const mode = (permissions && "inherited" in permissions ? providers.find((entry) => entry.id === permissions.provider) : provider)?.modes.find((entry) => entry.id === modeId) ?? null;
  const useLabel = action ? action(model ? modelAlias(model.id) : null) : model ? `Use ${modelAlias(model.id)}${effort ? ` · ${effort.label}` : ""}` : "Use model";
  const use = () => {
    if (onAuto && !pick) { onAuto(); return; }
    if (!pick || problem) return;
    const choice = { ...pick, modeId: permissions ? null : pick.modeId, featureValues: values?.kept ?? pick.featureValues };
    if (makeDefault) defaults?.make(choice);
    onUse(choice);
  };
  // Reset starts again from the Codex default as supplied, even on the model already checked: its effort and settings,
  // and Codex's default permissions. While that default cannot be chosen here, Reset clears nothing and says why.
  const codexRoute = codex && providers.find((entry) => entry.id === codex.provider && entry.available && entry.models.some((option) => option.id === codex.model));
  const resetUnavailable = codexRoute ? null : "The Codex default is not available on this host now, so Reset would leave a different model chosen.";
  const reset = () => {
    if (!codex || !codexRoute) return;
    defaults?.reset(); setMakeDefault(false);
    setChanged(null);
    setPick({ provider: codex.provider, model: codex.model, effort: codex.effort, modeId: permissions ? null : codexRoute.defaultMode ?? codexRoute.modes[0]?.id ?? null, featureValues: codex.featureValues ?? {} });
  };
  const body = <>
    <View>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>Model & settings</Text>
      <Text style={muted}>{subtitle}</Text>
    </View>
    {/* The chosen model, its route and that pair's own levels are one group marked Selected, so a vendor browsed below never
        reads as chosen; only assistive technology hears the levels' name. The frame holds the model alone and the levels keep the
        picker's full width, so Codex's six stay on one row at 320 points; longer labels wrap and move the list by that row, and a
        model without levels keeps one row's height. */}
    <View testID="kontor-model-chosen" role="group" accessibilityLabel="Selected model" style={{ gap: 6 }}>
      <View testID="kontor-model-selected" style={{ flexDirection: "row", alignItems: "center", gap: 8, minHeight: 32, padding: 8, borderRadius: 8, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.surface1 }}>
        {model && provider ? <>
          <VendorMark vendor={modelVendor(model.id)} colors={colors} size={20} />
          <View style={{ flex: 1, minWidth: 0 }}>
            <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{model.label}</Text>
            <Text numberOfLines={1} style={muted}>{`${routeLabel(provider.id, provider.label)} · ${modelAlias(model.id)}`}</Text>
          </View>
        </> : <Text style={{ ...muted, flex: 1 }}>{onAuto && !pick ? "Auto: each seat's chain" : "No model chosen"}</Text>}
        {model || (onAuto && !pick) ? <Text testID="kontor-model-selected-tag" style={{ color: colors.foreground, fontSize: 11, fontWeight: "600", paddingHorizontal: 6, paddingVertical: 2, borderRadius: 4, backgroundColor: colors.surface2 }}>Selected</Text> : null}
      </View>
      {model ? <View testID="kontor-model-levels" style={{ minHeight: tall ? 44 : 30, justifyContent: "center" }}>
        {model.efforts.length ? <Choices bare dense label="Effort" colors={colors} tall={tall} selected={pick?.effort ?? null} onSelect={(next) => patch({ effort: next })} options={model.efforts} />
          : <Text style={muted}>No effort setting for this model.</Text>}
      </View> : null}
    </View>
    <ScrollView testID="kontor-model-vendors" horizontal showsHorizontalScrollIndicator={false} keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 0 }}>
      <View accessibilityRole="tablist" accessibilityLabel="Vendors" style={{ flexDirection: "row", gap: 4 }}>
        {[...vendors].map(([vendor, count]) => {
          // The browsed tab is outlined, never in the accent the selected model wears; the selected model's vendor carries its check.
          const selected = vendor === shownVendor, holds = !!model && modelVendor(model.id) === vendor;
          return <Pressable key={vendor} testID={`kontor-model-vendor-${vendor}`} accessibilityRole="tab" aria-selected={selected} accessibilityState={{ selected }}
            accessibilityLabel={`${vendorName(vendor)}, ${count} ${count === 1 ? "model" : "models"}${holds ? ", has the selected model" : ""}`} {...NO_PRESS_DELAY} onPress={() => { setBrowsing(vendor); setRevealed(MODEL_BATCH); }}
            style={({ pressed }: { pressed: boolean }) => ({ minWidth: 56, maxWidth: 76, minHeight: tall ? 48 : 44, alignItems: "center", justifyContent: "center", gap: 3, paddingHorizontal: 6, borderRadius: 8,
              borderWidth: selected ? 2 : 1, borderColor: selected ? colors.foreground : colors.border, ...pressFeedback(colors, pressed, selected) })}>
            <VendorMark vendor={vendor} colors={colors} size={20} />
            <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 10, lineHeight: 13 }}>{vendorShortName(vendor)}</Text>
            {holds ? <View testID="kontor-model-holds" importantForAccessibility="no" accessibilityElementsHidden style={{ position: "absolute", top: 2, right: 2 }}>
              <Icon name="CircleCheck" size={12} color={readableInk(colors.accent, colors)} /></View> : null}
          </Pressable>;
        })}
      </View>
    </ScrollView>
    {/* The search names the vendor being browsed, so the list below reads as browsing, not as the choice above. */}
    <TextInput testID="kontor-model-search" accessibilityLabel={shownVendor ? `Search ${vendorName(shownVendor)} models` : "Search models"} placeholder={shownVendor ? `Search ${vendorName(shownVendor)} models…` : "Search models…"} value={query} onChangeText={(next: string) => { setQuery(next); setRevealed(MODEL_BATCH); }} maxLength={200}
      placeholderTextColor={colors.foregroundMuted} style={{ color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
    {/* The list has no scroller of its own: it shows whole rows, and more adds the next batch below them, so the
        settings after it stay in reach on the picker's one scroll. */}
    <View testID="kontor-model-list" style={{ gap: 4 }}>
      <View accessibilityRole="radiogroup" accessibilityLabel="Model" style={{ gap: GROUP_GAP }}>
        {onAuto ? <Pressable testID="kontor-model-auto" accessibilityRole="radio" aria-checked={!pick} accessibilityState={{ checked: !pick }} accessibilityLabel="Auto, use each seat's chain"
          {...NO_PRESS_DELAY} {...(Platform.OS === "web" ? spaceActivates(() => setPick(null)) : {})} onPress={() => setPick(null)} style={({ pressed }: { pressed: boolean }) => ({ minHeight: rowHeight, flexDirection: "row", alignItems: "center", gap: 10, paddingHorizontal: 8, borderRadius: 6, ...pressFeedback(colors, pressed, !pick) })}>
          <Icon name="RefreshCw" size={18} color={readableInk(colors.accent, colors)} />
          <View style={{ flex: 1, minWidth: 0 }}>
            <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>Auto</Text>
            <Text numberOfLines={1} style={muted}>Use each seat's chain</Text>
          </View>
          <Icon name={!pick ? "CircleCheck" : "Circle"} size={18} color={!pick ? readableInk(colors.accent, colors) : colors.foregroundMuted} />
        </Pressable> : null}
        {shown.length ? shown.map((group) => <View key={group.provider.id} style={{ gap: ROW_GAP }}>
          <Text style={{ ...muted, fontWeight: "600" }}>{group.provider.label}</Text>
          {group.rows.map((row) => {
            const checked = group.provider.id === pick?.provider && row.model.id === pick.model;
            return <Pressable key={row.model.id} testID={`kontor-model-row-${group.provider.id}/${row.model.id}`} accessibilityRole="radio" aria-checked={checked} accessibilityState={{ checked }}
              accessibilityLabel={`${row.model.label}, ${row.model.id}, ${group.route}${row.isCodex ? ", Codex default" : ""}`} {...NO_PRESS_DELAY} {...(Platform.OS === "web" ? spaceActivates(() => choose(group.provider, row.model)) : {})} onPress={() => choose(group.provider, row.model)}
              style={({ pressed }: { pressed: boolean }) => ({ minHeight: rowHeight, flexDirection: "row", alignItems: "center", gap: 10, paddingHorizontal: 8, borderRadius: 6, ...pressFeedback(colors, pressed, checked) })}>
              <VendorMark vendor={modelVendor(row.model.id)} colors={colors} size={20} />
              <View style={{ flex: 1, minWidth: 0 }}>
                <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{row.model.label}</Text>
                <Text numberOfLines={1} style={muted}>{group.route}{row.isCodex ? " · Codex default" : ""}</Text>
              </View>
              <Text style={{ color: colors.foreground, fontSize: 11, paddingHorizontal: 6, paddingVertical: 2, borderWidth: 1, borderColor: colors.border, borderRadius: 4 }}>{row.alias}</Text>
              <Icon name={checked ? "CircleCheck" : "Circle"} size={18} color={checked ? readableInk(colors.accent, colors) : colors.foregroundMuted} />
            </Pressable>;
          })}
        </View>) : <Text style={muted}>{tokens.length && shownVendor ? `No ${vendorName(shownVendor)} model matches “${query.trim()}”.` : "No available provider lists a model."}</Text>}
      </View>
      {hidden ? <Pressable testID="kontor-model-more" accessibilityRole="button" accessibilityLabel={`Show ${Math.min(hidden, MODEL_BATCH)} more ${tokens.length ? "matching " : ""}models, ${hidden} not shown`}
        {...NO_PRESS_DELAY} onPress={() => setRevealed(revealed + MODEL_BATCH)}
        style={({ pressed }: { pressed: boolean }) => ({ alignSelf: "center", minWidth: 44, minHeight: 44, paddingHorizontal: 16, alignItems: "center", justifyContent: "center", borderRadius: 8, ...pressFeedback(colors, pressed, false) })}>
        <Text style={{ color: colors.foreground, fontSize: 12 }}>more</Text>
      </Pressable> : null}
    </View>
    <View testID="kontor-model-settings" style={{ gap: 4, paddingTop: 6, paddingBottom: 8, borderTopWidth: 1, borderColor: colors.border }}>
      {model && provider ? <View style={{ gap: 4 }}>
        {onAuto ? <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{`Settings for ${model.label}`}</Text> : null}
        {!settings ? <Text style={muted}>Reading this model's settings…</Text>
          : settings.problem ? <Text testID="kontor-model-settings-problem" style={muted}>Model settings could not be read: {settings.problem}</Text>
          : settings.features.map((feature) => feature.type === "select"
            ? <Choices key={feature.id} layout="inline" label={feature.label} colors={colors} tall={tall} selected={featureValue(feature, pick?.featureValues ?? {}) as string | null}
              options={feature.options} onSelect={(next) => patch({ featureValues: { ...pick?.featureValues, [feature.id]: next } })} /> : null)}
        {permissions ? <View testID="kontor-model-permissions-inherited" style={{ gap: 2 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Icon name="Lock" size={14} color={colors.foregroundMuted} />
            <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{"seats" in permissions ? "Permissions inherited per seat" : "Permissions"}</Text>
          </View>
          {"seats" in permissions ? null : <Text testID="kontor-model-permissions-mode" style={{ color: colors.foreground, fontSize: 12 }}>{`${permissions.inherited === null ? "Inherited (unknown)" : `${mode?.label ?? permissions.inherited}${mode?.dangerous ? " ⚠" : ""}`} · ${providers.find((entry) => entry.id === permissions.provider)?.label ?? permissions.provider}`}</Text>}
          <Text style={muted}>Inherited from the seat; takeover never changes permissions.</Text>
        </View> : provider.modes.length ? <Disclosure testID="kontor-model-permissions" label="Permissions" value={mode ? `${mode.label}${mode.dangerous ? " ⚠" : ""}` : "Choose"}
          open={open === "permissions"} onToggle={() => toggle("permissions")} colors={colors} tall={tall}>
          <Choices bare label="Permissions" colors={colors} tall={tall} selected={pick?.modeId ?? null} onSelect={(next) => patch({ modeId: next })}
            options={provider.modes.map((entry) => ({ id: entry.id, label: entry.dangerous ? `${entry.label} ⚠` : entry.label }))} />
        </Disclosure> : null}
        {mode?.dangerous ? <Text testID="kontor-model-danger" style={{ color: colors.statusWarning, fontSize: 12 }}>⚠ {mode.label} is a dangerous permission mode on this host.</Text> : null}
        {/* Toggles follow permissions, as the board orders them; selects stand inline above. */}
        {settings && !settings.problem ? settings.features.map((feature) => feature.type === "toggle"
          ? <Toggle key={feature.id} label={feature.label} colors={colors} tall={tall} checked={featureValue(feature, pick?.featureValues ?? {}) === true}
            onPress={() => patch({ featureValues: { ...pick?.featureValues, [feature.id]: featureValue(feature, pick?.featureValues ?? {}) !== true } })} /> : null) : null}
      </View> : null}
      {changed || values?.dropped.length ? <Text testID="kontor-model-changed" accessibilityLiveRegion="polite" style={muted}>{[changed,
        values?.dropped.length ? `${model?.label ?? "This model"} does not offer ${values.dropped.join(", ")}, so ${values.dropped.length === 1 ? "it is" : "they are"} not applied.` : null].filter(Boolean).join(" ")}</Text> : null}
      {pick && problem ? <Text testID="kontor-model-problem" accessibilityLiveRegion="polite" style={muted}>{problem}</Text> : null}
      {defaults ? <View style={{ gap: 2 }}>
        <Text testID="kontor-model-default" style={muted}>New-lead default: {defaults.name}</Text>
        <Pressable testID="kontor-model-make-default" accessibilityRole="checkbox" accessibilityLabel="Make default for new leads" accessibilityHint="Otherwise, this lead only."
          accessibilityState={{ checked: makeDefault }} aria-checked={makeDefault} {...(Platform.OS === "web" ? spaceActivates(() => setMakeDefault(!makeDefault)) : {})} onPress={() => setMakeDefault(!makeDefault)} style={{ minHeight: 44, flexDirection: "row", alignItems: "center", gap: 10 }}>
          <View style={{ width: 20, height: 20, borderRadius: 5, borderWidth: 2, alignItems: "center", justifyContent: "center",
            borderColor: makeDefault ? colors.foreground : colors.foregroundMuted, backgroundColor: makeDefault ? colors.foreground : undefined }}>
            {makeDefault ? <Icon name="Check" size={14} color={colors.surface0} /> : null}
          </View>
          <View style={{ flex: 1 }}>
            <Text style={{ color: colors.foreground, fontSize: 13 }}>Make default for new leads</Text>
            <Text style={muted}>Otherwise, this lead only.</Text>
          </View>
        </Pressable>
        <Pressable testID="kontor-model-reset" accessibilityRole="button" accessibilityLabel={resetUnavailable ? `Reset to Codex default. ${resetUnavailable}` : "Reset to Codex default"} onPress={reset}
          disabled={!!resetUnavailable} aria-disabled={!!resetUnavailable || undefined} accessibilityState={{ disabled: !!resetUnavailable }}
          style={{ minHeight: tall ? 44 : 30, justifyContent: "center", alignSelf: "flex-start", opacity: resetUnavailable ? 0.5 : 1 }}>
          <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12 }}>Reset to Codex default</Text>
        </Pressable>
        {resetUnavailable ? <Text testID="kontor-model-reset-reason" style={muted}>{resetUnavailable}</Text> : null}
      </View> : null}
    </View>
    {note ? <Text testID="kontor-model-note" style={muted}>{note}</Text> : null}
  </>;
  // One vertical scroll for the whole picker: the surface's own where it scrolls, else the picker's, above its footer.
  return <View testID="kontor-model-picker" style={{ flexShrink: 1, minHeight: 0, gap: 8, padding: 6 }}>
    {hostScrolls ? body : <ScrollView testID="kontor-model-scroll" keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }} contentContainerStyle={{ gap: 8 }}>{body}</ScrollView>}
    {/* Board D2: an operation's picker ends in the shared confirmation footer. */}
    {onAuto ? <SheetFooter colors={colors} secondary={{ label: "Cancel", accessibilityLabel: "Cancel model choice", onPress: onCancel }}
      primary={{ testID: "kontor-model-use", label: useLabel, disabled: !!problem, hint: problem, onPress: use }} />
    : <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8, borderTopWidth: 1, borderColor: colors.border, paddingTop: 6 }}>
      <Action small label="Cancel" accessibilityLabel="Cancel model choice" colors={colors} onPress={onCancel} />
      <Pressable testID="kontor-model-use" accessibilityRole="button" accessibilityLabel={useLabel} accessibilityHint={problem ?? undefined} accessibilityState={{ disabled: !!problem }} aria-disabled={!!problem}
        disabled={!!problem} onPress={use} style={{ minHeight: tall ? 44 : 30, paddingHorizontal: 12, borderRadius: 6, justifyContent: "center", backgroundColor: problem ? colors.surface2 : colors.accent, opacity: problem ? 0.6 : 1 }}>
        <Text style={{ color: problem ? colors.foregroundMuted : colors.accentForeground, fontSize: 12, fontWeight: "600" }}>{useLabel}</Text>
      </Pressable>
    </View>}
  </View>;
}

/** An on/off setting; the knob's side and the switch state say it, never color alone. */
function Toggle({ label, checked, onPress, colors, tall }: { label: string; checked: boolean; onPress: () => void; colors: Colors; tall: boolean }) {
  return <Pressable accessibilityRole="switch" accessibilityLabel={label} accessibilityState={{ checked }} aria-checked={checked} {...(Platform.OS === "web" ? spaceActivates(onPress) : {})} onPress={onPress}
    style={{ minHeight: tall ? 44 : 32, flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
    <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{label}</Text>
    {/* The off track keeps a muted border and knob, so it stays visible on a light surface (3:1 against it). */}
    <View testID={`kontor-toggle-track-${label}`} style={{ width: 36, height: 20, borderRadius: 10, padding: 1, borderWidth: 1, borderColor: checked ? colors.accent : colors.foregroundMuted,
      alignItems: checked ? "flex-end" : "flex-start", backgroundColor: checked ? colors.accent : colors.surface2 }}>
      <View style={{ width: 16, height: 16, borderRadius: 8, backgroundColor: checked ? colors.surface0 : colors.foregroundMuted }} />
    </View>
  </Pressable>;
}

/**
 * A labelled single choice as radio chips; an unavailable option stays visible with its reason.
 * - `wrap`: chips wrap under the label.
 * - `segmented`: parts filling one row under the label, each at least its label's width, with `caption` explaining the
 *   chosen one (board B1). A part that no longer fits, as with large text, moves to the next row rather than being cut.
 *   `captionShown: false` keeps the caption's space, unseen and unspoken, for one that comes and goes.
 * - `inline`: the label, then the chips at its right on the same line (board B2).
 * `bare` leaves the label to the control that opened the choice.
 */
export function Choices({ label, options, selected, onSelect, colors, tall, empty, layout = "wrap", bare = false, dense = false, caption = null, captionShown = true }: {
  label: string; options: readonly { id: string; label: string; disabled?: boolean; note?: string | null }[]; selected: string | null;
  onSelect: (id: string) => void; colors: Colors; tall: boolean; empty?: string;
  layout?: "wrap" | "segmented" | "inline"; bare?: boolean; dense?: boolean; caption?: string | null; captionShown?: boolean;
}) {
  const enabled = options.filter((option) => !option.disabled);
  const tabStop = enabled.find((option) => option.id === selected)?.id ?? enabled[0]?.id;
  const inline = layout === "inline", parts = layout === "segmented";
  return <View accessibilityRole="radiogroup" accessibilityLabel={label} style={{ gap: 4, ...(inline ? { flexDirection: "row", alignItems: "center" } : {}) }}>
    {bare ? null : <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600", ...(inline ? { flex: 1 } : {}) }}>{label}</Text>}
    {options.length ? <View style={{ flexDirection: "row", flexWrap: inline ? "nowrap" : "wrap", gap: parts || dense ? 4 : 6 }}>
      {options.map((option) => {
        const checked = option.id === selected, disabled = !!option.disabled;
        return <Pressable key={option.id} accessibilityRole="radio" accessibilityLabel={option.note ? `${option.label}, ${option.note}` : option.label} accessibilityState={{ checked, disabled }} aria-checked={checked} aria-disabled={disabled}
          {...(Platform.OS === "web" ? { tabIndex: !disabled && option.id === tabStop ? 0 : -1, onKeyDown: (event: {
            key: string; repeat?: boolean; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean;
            currentTarget: { parentElement: { querySelectorAll(selector: string): ArrayLike<{ focus(): void }> } | null };
            preventDefault(): void; stopPropagation(): void;
          }) => {
            if (disabled || event.altKey || event.ctrlKey || event.metaKey) return;
            if (event.key === " " || event.key === "Spacebar") {
              event.preventDefault(); event.stopPropagation();
              if (!event.repeat) onSelect(option.id);
              return;
            }
            if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
            event.preventDefault(); event.stopPropagation();
            const index = enabled.findIndex((choice) => choice.id === option.id);
            const step = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
            const next = enabled[event.key === "Home" ? 0 : event.key === "End" ? enabled.length - 1 : (index + step + enabled.length) % enabled.length];
            if (!next) return;
            onSelect(next.id);
            event.currentTarget.parentElement?.querySelectorAll('[role="radio"]')[options.findIndex((choice) => choice.id === next.id)]?.focus();
          } } : {})}
          disabled={disabled} onPress={() => onSelect(option.id)} style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: tall ? 44 : 30, maxWidth: "100%", paddingHorizontal: dense ? 6 : 10, paddingVertical: 4, borderRadius: 6, borderWidth: 1, justifyContent: "center",
            ...(parts ? { flexGrow: 1, flexShrink: 1, flexBasis: "auto", alignItems: "center" } : {}),
            borderColor: checked || focused ? colors.accent : colors.border, backgroundColor: checked ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 })}>
          <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: checked ? "600" : "400" }}>{option.label}</Text>
        </Pressable>;
      })}
    </View> : <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{empty}</Text>}
    {/* A caption that comes and goes keeps its wrapped height while hidden, so a choice never moves what follows. */}
    {caption ? <Text testID={`kontor-choice-caption-${label}`} aria-hidden={!captionShown} accessibilityElementsHidden={!captionShown} importantForAccessibility={captionShown ? "auto" : "no-hide-descendants"}
      style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 14, opacity: captionShown ? 1 : 0 }}>{caption}</Text> : null}
  </View>;
}

/** One setting on one line, its name and current value; pressing it opens its choices below it (board B2). */
export function Disclosure({ label, value, open, onToggle, colors, tall, testID, children, ref }: {
  label: string; value: string; open: boolean; onToggle: () => void; colors: Colors; tall: boolean; testID?: string; children: React.ReactNode; ref?: React.Ref<View>;
}) {
  return <View style={{ gap: 4 }}>
    <Pressable ref={ref} testID={testID} accessibilityRole="button" accessibilityLabel={`${label}: ${value}`} aria-expanded={open} accessibilityState={{ expanded: open }} onPress={onToggle}
      style={({ pressed }: { pressed: boolean }) => ({ minHeight: tall ? 44 : 32, flexDirection: "row", alignItems: "center", gap: 8, borderRadius: 6, ...pressFeedback(colors, pressed) })}>
      <Text style={{ flex: 1, color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{label}</Text>
      <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foregroundMuted, fontSize: 12 }}>{value}</Text>
      <Icon name={open ? "ChevronDown" : "ChevronRight"} size={14} color={colors.foregroundMuted} />
    </Pressable>
    {open ? children : null}
  </View>;
}
