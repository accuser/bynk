// #847: hand-mirrored TS types for `bynk-lsp`'s `bynk/documentationModel` wire
// shape (`bynk-lsp/src/documentation_request.rs`). Maintained by hand alongside
// the Rust side, the same posture `webview/types.ts` takes for the sequence
// model — no LSP-payload codegen exists in this repo.

export interface Position {
  line: number;
  character: number;
}

export interface Range {
  start: Position;
  end: Position;
}

/** One declaration on the page. `markdown` is a fenced `bynk` signature plus,
 *  when `documented`, the declaration's doc-comment prose — rendered
 *  HTML-disabled by the webview. */
export interface DocEntry {
  name: string;
  kind: string;
  depth: number;
  markdown: string;
  documented: boolean;
  range: Range;
}

export interface DocModel {
  unitName: string;
  unitKind: string;
  unitDoc: string | null;
  unitRange: Range;
  entries: DocEntry[];
}

/** The payload the extension host embeds into the webview — the model plus the
 *  document URI every range in it is relative to. */
export interface DocPayload {
  model: DocModel;
  uri: string;
}
