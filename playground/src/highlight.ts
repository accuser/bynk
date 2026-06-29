// A lightweight CodeMirror 6 stream highlighter for Bynk.
//
// NOTE (in-browser track Q4): the intended end-state is highlighting from
// `tree-sitter-bynk` compiled to web-tree-sitter wasm — the grammar + queries
// already exist (`tree-sitter-bynk/queries/highlights.scm`) and `scripts/
// build-grammar.sh` builds the wasm. That build needs `emcc` or a running docker
// daemon, which the slice-4 environment lacks, so the MVP ships this faithful
// stream highlighter and the web-tree-sitter integration is the named follow-on.

import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";

const KEYWORDS = new Set([
  "context", "commons", "adapter", "agent", "service", "test", "integration",
  "capability", "type", "enum", "fn", "let", "match", "store", "from", "on",
  "consumes", "uses", "exports", "provides", "given", "binding", "by", "where",
  "transparent", "return", "if", "else", "for", "in", "as",
]);

// Built-in type / kind names and well-known constructors.
const TYPES = new Set([
  "String", "Int", "Float", "Bool", "Effect", "Result", "Option", "Query",
  "Stream", "List", "Map", "Set", "Instant", "Duration", "Uuid", "Connection",
  "Request", "Response", "Method", "FetchError", "ValidationError",
]);
const ATOMS = new Set([
  "Ok", "Err", "Some", "None", "true", "false", "Get", "Post", "Put", "Delete",
]);

const bynk = StreamLanguage.define({
  name: "bynk",
  startState: () => ({}),
  token(stream) {
    if (stream.eatSpace()) return null;

    // Line + block comments.
    if (stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }
    // Doc blocks delimited by `---` lines are highlighted as comments line-wise.
    if (stream.sol() && stream.match(/^---\s*$/)) return "comment";

    // Strings (double-quoted, with escapes).
    if (stream.match(/^"(?:[^"\\]|\\.)*"/)) return "string";
    if (stream.peek() === '"') {
      stream.next();
      while (!stream.eol()) {
        if (stream.next() === '"') break;
      }
      return "string";
    }

    // Numbers.
    if (stream.match(/^\d+(\.\d+)?/)) return "number";

    // Identifiers / keywords / types.
    const word = stream.match(/^[A-Za-z_][A-Za-z0-9_]*/);
    if (word) {
      // `match(regex)` returns the match array at runtime (the `true` arm is the
      // string-arg overload); index 0 is the matched word.
      const w = (word as RegExpMatchArray)[0];
      if (KEYWORDS.has(w)) return "keyword";
      if (ATOMS.has(w)) return "atom";
      if (TYPES.has(w) || /^[A-Z]/.test(w)) return "typeName";
      return "variableName";
    }

    // Operators / punctuation worth tinting.
    if (stream.match(/^(<-|->|=>|\+\+|::|[=+\-*/<>!&|?:.,;])/)) return "operator";

    stream.next();
    return null;
  },
});

// A dark highlight theme matching the playground shell.
const style = HighlightStyle.define([
  { tag: t.keyword, color: "#c792ea" },
  { tag: t.typeName, color: "#82aaff" },
  { tag: t.atom, color: "#f78c6c" },
  { tag: t.string, color: "#c3e88d" },
  { tag: t.number, color: "#f78c6c" },
  { tag: t.comment, color: "#5c6370", fontStyle: "italic" },
  { tag: t.operator, color: "#89ddff" },
  { tag: t.variableName, color: "#eeffff" },
]);

export function bynkHighlighting(): Extension {
  return [bynk, syntaxHighlighting(style)];
}
