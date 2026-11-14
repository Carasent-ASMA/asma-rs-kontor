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
  if (/token|Codex Auto Review|cache miss|cache read|cache write|^output$|reasoning|code (added|deleted)|tests (added|deleted)|docs (added|deleted)|lines|rework/i.test(label) && !/per|1K|share|%/i.test(label)) return reportNumber(value);
  if (/%|churn/i.test(label)) return `${value.toFixed(1)}%`;
  if (/time to merge/i.test(label)) return value < 48 ? `${value.toFixed(1)} h` : `${(value / 24).toFixed(1)} d`;
  if (/seat.hours|open hours|active|orchestrator/i.test(label)) return `${value.toFixed(1)}${/merge.*\(h\)/i.test(label) ? "" : " h"}`;
  if (/per 1K|\/ 1K/i.test(label)) return value.toFixed(2);
  return value.toLocaleString("en-US", { maximumFractionDigits: Number.isInteger(value) ? 0 : 1 });
}
export function reportNote(text: string): string {
  return text.replace(/\$([\d,]+(?:\.\d+)?)/g, (_, n: string) => reportMoney(Number(n.replaceAll(",", ""))))
    .replace(/(?<![\d\w-])(\d{7,})(?![\d\w-])/g, (n) => reportNumber(Number(n)))
    .replace(/(Δ\s+[-+]?\d+)\.(\d{2})\d+/g, "$1.$2");
}
