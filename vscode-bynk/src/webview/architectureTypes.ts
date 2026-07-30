// #851: hand-mirrored TS types for `bynk-lsp`'s `bynk/architectureModel` wire
// shape (`bynk-lsp/src/architecture_request.rs`). No codegen exists in this
// repo for LSP payload types, so this is maintained by hand alongside the
// Rust side — the same posture `webview/types.ts` (#846) and `webview/
// docTypes.ts` (#847) take.
//
// Unlike both siblings' wire shapes, this one is project-scoped: there is no
// single document URI the whole payload is relative to, so every location
// here (`ArchLoc`) carries its own `uri` alongside its `range`.

export type NodeKind = "Context" | "Adapter";

export interface Position {
  line: number;
  character: number;
}

export interface Range {
  start: Position;
  end: Position;
}

/** A click-to-code target: a file URI plus the range within it. */
export interface ArchLoc {
  uri: string;
  range: Range;
}

export interface ArchCapability {
  name: string;
  local: boolean;
  /** The providing unit's qualified name, for a consumed (non-local)
   *  capability — `"bynk"` for the toolchain's built-in surface. `null` for a
   *  locally-declared capability. */
  from: string | null;
  loc: ArchLoc;
}

export interface ArchProvider {
  capability: string;
  providerName: string;
  external: boolean;
  loc: ArchLoc;
}

export interface ArchService {
  name: string;
  handlerCount: number;
  loc: ArchLoc;
}

export interface ArchAgent {
  name: string;
  handlerCount: number;
  loc: ArchLoc;
}

export interface ArchNode {
  name: string;
  kind: NodeKind;
  loc: ArchLoc;
  capabilities: ArchCapability[];
  providers: ArchProvider[];
  services: ArchService[];
  agents: ArchAgent[];
}

export interface ArchEdge {
  from: string;
  to: string;
  /** Selected capability labels (braced `consumes` form); empty for a
   *  whole-unit consumes. */
  capabilities: string[];
  loc: ArchLoc;
}

export interface ArchModel {
  nodes: ArchNode[];
  edges: ArchEdge[];
}

/** The payload the extension host embeds into the webview. Unlike
 *  `SequencePayload`/`DocPayload`, there is no single `uri` — every location
 *  in the model already carries its own (see the module doc). */
export interface ArchPayload {
  model: ArchModel;
}

/** A click-to-code message the webview posts back to the extension host —
 *  the same shape `RevealMessage` (`webview/types.ts`) uses, kept as its own
 *  type here so this module has no import dependency on the sequence view's. */
export interface ArchRevealMessage {
  type: "reveal";
  uri: string;
  range: Range;
}
