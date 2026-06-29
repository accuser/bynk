// web-tree-sitter syntax highlighting for the playground (in-browser track, slice 5b).
//
// Loads `tree-sitter-bynk` compiled to wasm (the same grammar the editor/CLI use) and
// its `highlights.scm` query, parses the document, and renders the query captures as
// CodeMirror mark decorations. Loading is async (two wasm fetches), so the editor
// starts on the stream highlighter (highlight.ts) and swaps to this once ready;
// `bynkTreeSitterHighlighting()` returns `null` if the wasm can't load, leaving the
// fallback in place. The grammar query string is bundled at build time (esbuild
// `.scm` text loader); the two wasm blobs are fetched from the deploy root.

import { Decoration, EditorView, ViewPlugin } from "@codemirror/view";
import type { DecorationSet, ViewUpdate } from "@codemirror/view";
import { RangeSetBuilder } from "@codemirror/state";
import type { Extension } from "@codemirror/state";
import Parser from "web-tree-sitter";
// esbuild loads this as a string (the `.scm: text` loader in build.mjs).
import highlightsQuery from "./vendor/highlights.scm";

// Map a tree-sitter capture name (e.g. `keyword.import`) to a CSS class by its head
// (`keyword`). Heads not listed are left unhighlighted.
const CLASS_FOR: Record<string, string> = {
  keyword: "tok-keyword",
  operator: "tok-operator",
  type: "tok-type",
  constructor: "tok-type",
  string: "tok-string",
  number: "tok-number",
  comment: "tok-comment",
  function: "tok-function",
  module: "tok-namespace",
  namespace: "tok-namespace",
  variable: "tok-variable",
  property: "tok-property",
  constant: "tok-constant",
  punctuation: "tok-punct",
};

function classFor(capture: string): string | null {
  return CLASS_FOR[capture.split(".")[0]] ?? null;
}

// Colours for the classes above — the same palette as the stream highlighter.
const tsTheme = EditorView.theme({
  ".tok-keyword": { color: "#c792ea" },
  ".tok-type": { color: "#82aaff" },
  ".tok-constant": { color: "#f78c6c" },
  ".tok-string": { color: "#c3e88d" },
  ".tok-number": { color: "#f78c6c" },
  ".tok-comment": { color: "#5c6370", fontStyle: "italic" },
  ".tok-operator": { color: "#89ddff" },
  ".tok-function": { color: "#82aaff" },
  ".tok-namespace": { color: "#ffcb6b" },
  ".tok-property": { color: "#eeffff" },
  ".tok-variable": { color: "#eeffff" },
  ".tok-punct": { color: "#89ddff" },
});

async function initParser(): Promise<{ parser: Parser; query: Parser.Query } | null> {
  try {
    await Parser.init({ locateFile: () => new URL("tree-sitter.wasm", location.href).href });
    const lang = await Parser.Language.load(new URL("tree-sitter-bynk.wasm", location.href).href);
    const parser = new Parser();
    parser.setLanguage(lang);
    return { parser, query: lang.query(highlightsQuery) };
  } catch {
    return null;
  }
}

/**
 * Build the web-tree-sitter highlighting extension, or `null` if the grammar wasm
 * could not be loaded (caller keeps the stream-highlighter fallback).
 */
export async function bynkTreeSitterHighlighting(): Promise<Extension | null> {
  const ready = await initParser();
  if (!ready) return null;
  const { parser, query } = ready;

  const decorate = (view: EditorView): DecorationSet => {
    const tree = parser.parse(view.state.doc.toString());
    if (!tree) return Decoration.none;
    const builder = new RangeSetBuilder<Decoration>();
    // Sort by start; apply greedily so ranges stay ordered and non-overlapping
    // (RangeSetBuilder requires that; nested captures collapse to the outer one).
    const caps = query.captures(tree.rootNode).slice();
    caps.sort((a, b) => a.node.startIndex - b.node.startIndex);
    let last = 0;
    for (const c of caps) {
      const cls = classFor(c.name);
      if (!cls) continue;
      const from = c.node.startIndex;
      const to = c.node.endIndex;
      if (from < last || to <= from) continue;
      builder.add(from, to, Decoration.mark({ class: cls }));
      last = to;
    }
    tree.delete();
    return builder.finish();
  };

  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = decorate(view);
      }
      update(u: ViewUpdate) {
        if (u.docChanged) this.decorations = decorate(u.view);
      }
    },
    { decorations: (v) => v.decorations },
  );

  return [plugin, tsTheme];
}
