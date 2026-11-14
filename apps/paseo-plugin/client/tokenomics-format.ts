// Display rules from the report; raw accounting and CSV values stay untouched.
export function reportNumber(value: number): string {
  const a = Math.abs(value);
  return a >= 1e9 ? `${(value / 1e9).toFixed(2)}B` : a >= 1e6 ? `${(value / 1e6).toFixed(1)}M` : a >= 1e3 ? `${(value / 1e3).toFixed(1)}K` : String(Math.round(value));
}
export const reportMoney = (value: number) => `$${value >= 100 ? value.toLocaleString("en-US", { maximumFractionDigits: 0 }) : value.toFixed(2)}`;
export function reportValue(value: string | number | null | undefined, label = ""): string {
  if (value == null) return "—";
  if (typeof value === "string") {
    if (/^\$[\d,.]+$/.test(value)) return reportMoney(Number(value.slice(1).replaceAll(",", "")));
    if (/^\d{4}-\d{2}-\d{2}T/.test(value)) return value.replace("T", " ").slice(0, 16);
    return value;
  }
  if (/cost|\$|API-equivalent|price/i.test(label) && !/share|%|rate|hours/i.test(label)) return reportMoney(value);
  if (/token|sub.thread|Codex Auto Review|input miss|cache miss|cache read|cache write|^output$|reasoning|code (added|deleted)|tests (added|deleted)|docs (added|deleted)|lines|rework/i.test(label) && !/per|1K|share|%/i.test(label)) return reportNumber(value);
  if (/\(pp\)/i.test(label)) return `${value.toFixed(1)} pp`;
  if (/%|churn/i.test(label)) return `${value.toFixed(1)}%`;
  if (/time to merge/i.test(label)) return value < 48 ? `${value.toFixed(1)} h` : `${(value / 24).toFixed(1)} d`;
  if (/seat.hours|open hours|active|orchestrator/i.test(label)) return `${value.toFixed(1)}${/merge.*\(h\)/i.test(label) ? "" : " h"}`;
  if (/Bugs introduced|Bugs fixed/i.test(label)) return value ? value.toFixed(1) : "0";
  if (/Findings.*(?:per|\/) PR|Avg parallel/i.test(label)) return value.toFixed(1);
  if (/Median PR size/i.test(label)) return reportNumber(value);
  if (/per 1K|\/ 1K/i.test(label)) return value.toFixed(2);
  return Number.isInteger(value) && !/responses|sessions/i.test(label) ? String(value) : value.toLocaleString("en-US", { maximumFractionDigits: Number.isInteger(value) ? 0 : 1 });
}
export function reportNote(text: string): string {
  return text.replace(/([+−]?\d[\d,]*(?:\.\d+)?)(?=\s*(?:deleted|lines|read ·|miss\b|tokens\b))/g, (s) => { const sign = s.startsWith("+") || s.startsWith("−") ? s[0] : ""; return sign + reportNumber(Number(s.replaceAll(",", "").replace("−", "-")) * (sign === "−" ? -1 : 1)); })
    .replace(/tests \+(\d+)\/−(\d+) · docs \+(\d+)\/−(\d+)/g, (_, a, b, c, d) => `tests +${reportNumber(Number(a))} / −${reportNumber(Number(b))} · docs +${reportNumber(Number(c))} / −${reportNumber(Number(d))}`)
    .replace(/\$([\d,]+(?:\.\d+)?)/g, (_, n: string) => n === "0" ? "$0" : reportMoney(Number(n.replaceAll(",", ""))))
    .replace(/(?<![\d\w-])(\d{7,})(?![\d\w-])/g, (n) => reportNumber(Number(n)))
    .replace(/(Δ\s+[-+]?\d+)\.(\d{2})\d+/g, "$1.$2");
}

export function reportLivePercent(value: string | number | null | undefined): string {
  if (value == null || !Number.isFinite(parseFloat(String(value)))) return "—";
  const percent = parseFloat(String(value));
  return `${percent.toFixed(percent && percent < 10 ? 1 : 0)}%`;
}

export const reportLinePair = (added: string | number | null | undefined, deleted: string | number | null | undefined): string => added == null || deleted == null || !Number(added) && !Number(deleted) ? "—" : `+${reportNumber(Number(added))} / −${reportNumber(Number(deleted))}`;

export function reportTableValue(value: string | number | null | undefined, label: string, table: string): string {
  if (/^(review-|reviewer$|submitters$|delivery-|jira-|bug-|bugs$)/.test(table) && (typeof value === "string" && value.endsWith("%") || typeof value === "number" && /%|Churn/i.test(label))) return reportLivePercent(value);
  if (table === "delivery-prs" && label === "Merged" && typeof value === "string") return value.slice(0, 10);
  if (table === "bug-people" && label === "Bugs" && typeof value === "string") { const keys = value ? value.split(", ") : []; return keys.slice(0, 6).join(", ") + (keys.length > 6 ? ` +${keys.length - 6}` : ""); }
  if (table === "coverage" && typeof value === "number" && !/hours/i.test(label)) return String(value);
  if (table === "prices") return value == null ? "—" : String(value);
  if (table === "all-months" && typeof value === "string" && /^\d+(?:\.\d+)? · \$/.test(value)) { const [tokens, cost] = value.split(" · "); return `${reportNumber(Number(tokens))} · ${reportMoney(Number(cost!.slice(1).replaceAll(",", "")))}`; }
  return reportValue(value, label);
}
