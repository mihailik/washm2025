// TypeScript-backed host: exposes `highlight` and `complete` callbacks that the
// reedline WASM bridge calls into through `host_highlight` / `host_complete`.
//
// Callers must provide a TypeScript module (`ts`). In Node:
//     import ts from "typescript";
// In the browser load it from a CDN, e.g.:
//     const ts = (await import("https://esm.sh/typescript@5.5.4")).default;

const REPL_FILE = "/repl.ts";
const LIB_FILE = "/lib.repl.d.ts";

// Tiny ambient declarations that give the language service something to talk
// about so completions feel useful without pulling in megabytes of lib files.
const LIB_SOURCE = `
declare const console: {
  log(...args: any[]): void;
  error(...args: any[]): void;
  warn(...args: any[]): void;
  info(...args: any[]): void;
  debug(...args: any[]): void;
  table(value: any): void;
};
declare const Math: {
  PI: number; E: number; LN2: number; LN10: number;
  abs(x: number): number;
  max(...args: number[]): number;
  min(...args: number[]): number;
  floor(x: number): number;
  ceil(x: number): number;
  round(x: number): number;
  sqrt(x: number): number;
  pow(x: number, y: number): number;
  random(): number;
  sin(x: number): number;
  cos(x: number): number;
  tan(x: number): number;
  log(x: number): number;
};
declare const JSON: {
  stringify(value: any, replacer?: any, space?: any): string;
  parse(text: string): any;
};
declare const Object: {
  keys(o: any): string[];
  values(o: any): any[];
  entries(o: any): [string, any][];
  assign(t: any, ...s: any[]): any;
  freeze<T>(o: T): T;
};
declare const Array: {
  isArray(o: any): boolean;
  from<T>(iter: Iterable<T>): T[];
  of<T>(...items: T[]): T[];
};
declare const Promise: any;
declare function setTimeout(handler: () => void, timeout: number): number;
declare function clearTimeout(handle: number): void;
declare function setInterval(handler: () => void, interval: number): number;
declare function clearInterval(handle: number): void;
declare function parseInt(s: string, radix?: number): number;
declare function parseFloat(s: string): number;
declare function isNaN(n: number): boolean;
declare function isFinite(n: number): boolean;
declare const undefined: any;
declare const NaN: number;
declare const Infinity: number;
declare const globalThis: any;
`;

export function createTypeScriptHost(ts) {
    let source = "";
    let version = 0;

    const host = {
        getScriptFileNames: () => [REPL_FILE, LIB_FILE],
        getScriptVersion: (n) => (n === REPL_FILE ? String(version) : "1"),
        getScriptSnapshot: (n) => {
            if (n === REPL_FILE) return ts.ScriptSnapshot.fromString(source);
            if (n === LIB_FILE) return ts.ScriptSnapshot.fromString(LIB_SOURCE);
            return undefined;
        },
        getCurrentDirectory: () => "/",
        getCompilationSettings: () => ({
            target: ts.ScriptTarget.Latest,
            allowJs: true,
            noLib: true, // we ship our own minimal ambient declarations above
            strict: false,
            module: ts.ModuleKind.ESNext,
        }),
        getDefaultLibFileName: () => LIB_FILE,
        fileExists: (n) => n === REPL_FILE || n === LIB_FILE,
        readFile: (n) => (n === LIB_FILE ? LIB_SOURCE : n === REPL_FILE ? source : ""),
        readDirectory: () => [],
        directoryExists: () => true,
        getDirectories: () => [],
    };

    let svc = null;
    try {
        svc = ts.createLanguageService(host);
    } catch (e) {
        console.warn("[ts-host] LanguageService init failed:", e);
    }
    const classifier = ts.createClassifier();

    // Map ts.TokenClass values to ANSI SGR codes.
    const STYLE_BY_CLASS = {
        [ts.TokenClass.Keyword]: 35, // magenta
        [ts.TokenClass.Operator]: 90, // bright black / gray
        [ts.TokenClass.Comment]: 90,
        [ts.TokenClass.NumberLiteral]: 33, // yellow
        [ts.TokenClass.BigIntLiteral]: 33,
        [ts.TokenClass.StringLiteral]: 32, // green
        [ts.TokenClass.RegExpLiteral]: 31, // red
    };

    function isWordChar(c) {
        return /[A-Za-z0-9_$]/.test(c);
    }

    return {
        highlight(line, _cursor) {
            const r = classifier.getClassificationsForLine(
                line,
                ts.EndOfLineState.None,
                false,
            );
            let out = "";
            let pos = 0;
            for (const e of r.entries) {
                const text = line.substr(pos, e.length);
                const code = STYLE_BY_CLASS[e.classification];
                out += code ? `\x1b[${code}m${text}\x1b[0m` : text;
                pos += e.length;
            }
            return out;
        },
        complete(line, cursor) {
            source = line;
            version++;
            let start = cursor;
            while (start > 0 && isWordChar(line[start - 1])) start--;
            const prefix = line.slice(start, cursor);
            if (!svc) return { start, end: cursor, suggestions: [] };
            try {
                const info = svc.getCompletionsAtPosition(REPL_FILE, cursor, {});
                if (!info) return { start, end: cursor, suggestions: [] };
                const lower = prefix.toLowerCase();
                const seen = new Set();
                const suggestions = [];
                for (const e of info.entries) {
                    if (lower && !e.name.toLowerCase().startsWith(lower)) continue;
                    if (seen.has(e.name)) continue;
                    seen.add(e.name);
                    suggestions.push(e.name);
                    if (suggestions.length >= 32) break;
                }
                return { start, end: cursor, suggestions };
            } catch (e) {
                return { start, end: cursor, suggestions: [] };
            }
        },
    };
}
