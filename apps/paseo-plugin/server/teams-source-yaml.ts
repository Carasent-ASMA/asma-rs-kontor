/** Dependency-free decoder for the configuration dialect used by the authorized fleet
 * and core roster files: block maps/lists, flow maps/lists, quoted/plain scalars and
 * backward aliases of flat scalar lists. It is deliberately not a general YAML loader.
 * Tags, directives, merge keys, complex keys, multiline scalars and recursive/compound
 * aliases are refused. No constructors, filesystem access or implicit YAML 1.1 booleans.
 */
type Value = string | number | boolean | null | Value[] | { [key: string]: Value };
type Line = { indent: number; text: string; number: number };

function withoutComment(text: string): string {
  let quote = "";
  for (let i = 0; i < text.length; i++) {
    const char = text[i];
    if (quote) {
      if (quote === '"' && char === "\\") { i++; continue; }
      if (char === quote) {
        if (quote === "'" && text[i + 1] === "'") { i++; continue; }
        quote = "";
      }
    } else if (char === '"' || char === "'") quote = char;
    else if (char === "#" && (i === 0 || /\s/.test(text[i - 1]!))) return text.slice(0, i).trimEnd();
  }
  if (quote) throw new Error("Unterminated YAML quote");
  return text.trimEnd();
}

class ConfigYamlDecoder {
  private readonly lines: Line[];
  private readonly anchors = new Map<string, Value[]>();
  private cursor = 0;
  private nodes = 0;

  constructor(text: string) {
    this.lines = text.split(/\r?\n/).flatMap((raw, index) => {
      if (/^\s*\t/.test(raw)) throw new Error(`YAML tabs are unsupported at line ${index + 1}`);
      const clean = withoutComment(raw);
      if (!clean.trim()) return [];
      return [{ indent: clean.length - clean.trimStart().length, text: clean.trimStart(), number: index + 1 }];
    });
  }

  read(): Value {
    if (!this.lines.length) throw new Error("Empty YAML source");
    const result = this.block(0, 0);
    if (this.cursor !== this.lines.length) throw new Error(`Unsupported YAML at line ${this.lines[this.cursor]?.number}`);
    return result;
  }

  private budget(depth: number): void {
    if (depth > 32 || ++this.nodes > 100_000) throw new Error("YAML nesting/expansion limit exceeded");
  }

  private mappingEntry(text: string): [string, string] {
    // Config keys are literal identifiers/paths; quoted, flow and merge keys are unsupported.
    const match = /^([a-zA-Z0-9_./-]+):(?:\s+(.*)|$)/.exec(text);
    if (!match?.[1]) throw new Error(`Unsupported YAML mapping: ${text.slice(0, 100)}`);
    return [match[1], match[2] ?? ""];
  }

  private set(map: { [key: string]: Value }, key: string, value: Value): void {
    if (Object.hasOwn(map, key)) throw new Error(`Duplicate YAML key: ${key}`);
    Object.defineProperty(map, key, { value, enumerable: true, writable: true, configurable: true });
  }

  private nested(rest: string, indent: number, depth: number): Value {
    if (rest) return this.inline(rest, depth + 1);
    const next = this.lines[this.cursor];
    if (next && (next.indent > indent || (next.indent === indent && next.text.startsWith("- ")))) {
      return this.block(next.indent, depth + 1);
    }
    return null;
  }

  private block(indent: number, depth: number): Value {
    this.budget(depth);
    const sequence = this.lines[this.cursor]?.text.startsWith("- ");
    const list: Value[] = [];
    const map: { [key: string]: Value } = {};
    while (this.cursor < this.lines.length) {
      const line = this.lines[this.cursor]!;
      if (line.indent < indent) break;
      if (line.indent !== indent) throw new Error(`Unexpected YAML indentation at line ${line.number}`);
      if (sequence && !line.text.startsWith("- ")) break; // indentationless roster sequence
      if (!sequence && line.text.startsWith("- ")) throw new Error(`Mixed YAML collection at line ${line.number}`);
      this.cursor++;
      if (!sequence) {
        const [key, rest] = this.mappingEntry(line.text);
        this.set(map, key, this.nested(rest, indent, depth));
        continue;
      }
      const rest = line.text.slice(2).trim();
      if (/^[a-zA-Z0-9_./-]+:(?:\s|$)/.test(rest)) {
        const item: { [key: string]: Value } = {};
        const [key, value] = this.mappingEntry(rest);
        this.set(item, key, this.nested(value, indent + 2, depth + 1));
        while (this.lines[this.cursor]?.indent === indent + 2) {
          const entry = this.lines[this.cursor++]!;
          const [nextKey, nextValue] = this.mappingEntry(entry.text);
          this.set(item, nextKey, this.nested(nextValue, indent + 2, depth + 1));
        }
        list.push(item);
      } else list.push(this.nested(rest, indent, depth));
    }
    return sequence ? list : map;
  }

  private inline(text: string, depth: number): Value {
    this.budget(depth);
    const anchor = /^&([a-zA-Z0-9_-]+)\s+(.+)$/.exec(text);
    if (anchor) {
      const name = anchor[1]!;
      const value = this.inline(anchor[2]!, depth + 1);
      if (!Array.isArray(value) || value.some((entry) => typeof entry === "object")) {
        throw new Error("Only flat scalar-list YAML anchors are supported");
      }
      if (this.anchors.has(name)) throw new Error(`Duplicate YAML anchor: ${name}`);
      this.anchors.set(name, value);
      return value;
    }
    if (text.startsWith("*")) {
      const value = this.anchors.get(text.slice(1));
      if (!value) throw new Error(`Unknown or unsupported YAML alias: ${text}`);
      this.nodes += value.length;
      return [...value];
    }
    if (text.startsWith("[") || text.startsWith("{")) return this.flow(text, depth + 1);
    if (text.startsWith('"')) {
      const parsed: unknown = JSON.parse(text);
      if (typeof parsed !== "string") throw new Error("Expected YAML string");
      return parsed;
    }
    if (text.startsWith("'")) {
      if (!/^'(?:[^']|'')*'$/.test(text)) throw new Error("Invalid single-quoted YAML string");
      return text.slice(1, -1).replaceAll("''", "'");
    }
    if (/^[!&*|>@`%?:,\]}]/.test(text) || /[\[\]{}]/.test(text)) throw new Error(`Unsupported YAML scalar: ${text.slice(0, 100)}`);
    if (text === "true") return true;
    if (text === "false") return false;
    if (text === "null" || text === "~") return null;
    if (/^-?(?:0|[1-9]\d*)(?:\.\d+)?$/.test(text)) return Number(text);
    return text;
  }

  private flow(text: string, depth: number): Value {
    const close = text.startsWith("[") ? "]" : "}";
    if (!text.endsWith(close)) throw new Error("Unterminated YAML flow collection");
    const inner = text.slice(1, -1);
    const entries: string[] = [];
    let nesting = 0;
    let quote = "";
    let start = 0;
    for (let i = 0; i < inner.length; i++) {
      const char = inner[i];
      if (quote) {
        if (quote === '"' && char === "\\") { i++; continue; }
        if (char === quote) {
          if (quote === "'" && inner[i + 1] === "'") { i++; continue; }
          quote = "";
        }
      } else if (char === '"' || char === "'") quote = char;
      else if (char === "[" || char === "{") nesting++;
      else if (char === "]" || char === "}") nesting--;
      else if (char === "," && nesting === 0) { entries.push(inner.slice(start, i).trim()); start = i + 1; }
      if (nesting < 0) throw new Error("Unbalanced YAML flow collection");
    }
    if (quote || nesting) throw new Error("Unbalanced YAML flow collection");
    const last = inner.slice(start).trim();
    if (last) entries.push(last);
    if (entries.some((entry) => !entry)) throw new Error("Empty YAML flow entry");
    if (close === "]") return entries.map((entry) => this.inline(entry, depth + 1));
    const map: { [key: string]: Value } = {};
    for (const entry of entries) {
      const [key, rest] = this.mappingEntry(entry);
      this.set(map, key, this.inline(rest, depth + 1));
    }
    return map;
  }
}

export function decodeTeamsConfigYaml(text: string): unknown {
  return new ConfigYamlDecoder(text).read();
}
