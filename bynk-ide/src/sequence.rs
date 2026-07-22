//! #846: the sequence-diagram query.
//!
//! Classifies a handler body's calls into runtime-participant lifelines —
//! consumed capabilities, calls into consumed contexts, and agents (including
//! same-context agents) — for the "Show Sequence Diagram" VS Code feature.
//! Everything else (commons fns, context-local fns, methods, constructors)
//! folds into the entry participant's own activation: no message is emitted
//! for the call, and — because this repo's resolver does not inline call
//! bodies either — a lifeline call written inside a commons `fn`'s own body is
//! invisible to this walk. That is a stated Tier-1 limitation, not a bug: see
//! `design/pending/sequence-diagram-846.md`.
//!
//! A pure, read-only IDE query: it never touches the checker's hot path
//! (`bynk_check::checker::check_handler_body`) and is built on the exhaustive
//! `expr_children`/`statement_exprs` walkers only insofar as this module's own
//! statement dispatch mirrors their coverage of [`Statement`] — a new
//! `Statement` variant is a compile error here, in `Builder::walk_block`'s
//! `match`, not a silent gap.
//!
//! Cross-context/agent calls are boundary-stop (Decision C): one `Call` +
//! one `Return` message: the callee's own body is never walked, even where
//! reachable (an agent's handlers are visible via [`ContextSequenceInfo::agents`]).

use bynk_emit::project::ContextSequenceInfo;
use bynk_syntax::ast::*;
use bynk_syntax::span::Span;

/// Which declaration owns the handler being diagrammed.
#[derive(Debug, Clone, Copy)]
pub enum HandlerOwner<'a> {
    Service(&'a str),
    Agent(&'a str),
}

/// Nesting budget for rendered `if`/`match` blocks (issue #846: "~2 levels").
/// Beyond this depth the walk stops classifying calls and emits a single
/// [`AltKind::Collapsed`] marker instead of recursing further.
const MAX_BLOCK_DEPTH: u32 = 2;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SequenceModel {
    pub participants: Vec<Participant>,
    pub messages: Vec<Message>,
    pub blocks: Vec<AltBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Participant {
    pub id: u32,
    pub kind: ParticipantKind,
    pub name: String,
    /// `None` for `Entry` — it has no single declaration site to jump to
    /// (it *is* the handler).
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticipantKind {
    Entry,
    Capability,
    Context,
    Agent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub from: u32,
    pub to: u32,
    pub kind: MessageKind,
    pub label: String,
    pub span: Span,
    pub block: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Call,
    Return,
    /// `~>` fire-and-forget — no paired `Return`.
    Send,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AltBlock {
    pub id: u32,
    pub kind: AltKind,
    /// Empty for `Collapsed`.
    pub branches: Vec<Branch>,
    pub span: Span,
    pub parent: Option<u32>,
    /// Which of `parent`'s branches this block is nested under — `None` iff
    /// `parent` is `None`. Needed to render nesting correctly: a parent
    /// branch can be entirely empty of messages (the rate-limiter's `if`/
    /// `else` gating only a return, for one), so a renderer walking
    /// `Message.block` alone would have no way to place a nested block whose
    /// own branches are also message-free.
    pub parent_branch: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltKind {
    If,
    Match,
    Collapsed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    pub label: String,
    pub message_ids: Vec<usize>,
}

/// Whether an expression sits directly under an effect operator
/// (`<-`/`~>`/`do`) — only then can it be a lifeline call, since a bare
/// (pure) `let` cannot bind an `Effect[_]` value in this language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrow {
    /// `let x <- expr` / `do expr` — awaited; a Call+Return pair.
    Awaited,
    /// `~> expr` — fire-and-forget; a Send only.
    FireAndForget,
}

const ENTRY_ID: u32 = 0;

/// The block/branch a walk is currently nested inside — `None` at the
/// handler-body top level. Threaded through every walk method so a nested
/// block can record `parent_branch` and a message can record its `block`.
#[derive(Debug, Clone, Copy)]
struct BlockCtx {
    id: u32,
    branch: u32,
}

/// `default_given` is the owning service's service-level `given` default
/// (v0.155, `ServiceDecl.default_given`) — a handler that declares no `given`
/// of its own inherits it. This classifier walks a freshly-parsed AST that
/// has *not* been through `bynk-emit`'s `inject_service_defaults`
/// normalization pass (which is what mutates `handler.given` in the compile
/// pipeline), so the fallback has to be applied here or a handler relying on
/// the service default would drop every capability lifeline. Pass `&[]` when
/// there is no default (always the case for `HandlerOwner::Agent` — agents
/// have no service-level `given`).
pub fn sequence_model(
    handler: &Handler,
    owner: HandlerOwner<'_>,
    default_given: &[CapRef],
    info: Option<&ContextSequenceInfo>,
) -> SequenceModel {
    let given = if handler.given.is_empty() {
        default_given
    } else {
        &handler.given
    };
    let mut b = Builder::new(entry_label(handler, owner), given, info);
    b.walk_block(&handler.body, None, 0);
    b.finish()
}

fn entry_label(handler: &Handler, owner: HandlerOwner<'_>) -> String {
    let discriminator = match &handler.kind {
        HandlerKind::Call => match handler.method_name.as_ref() {
            Some(m) => m.name.clone(),
            None => "call".to_string(),
        },
        HandlerKind::Http { method, path } => format!("{} {}", method.as_str(), path),
        HandlerKind::Cron { expr } => format!("cron \"{expr}\""),
        HandlerKind::Message => "message".to_string(),
        HandlerKind::Open => "open".to_string(),
        HandlerKind::Close => "close".to_string(),
    };
    match owner {
        HandlerOwner::Service(name) => format!("{name} {discriminator}"),
        HandlerOwner::Agent(name) => format!("{name}.{discriminator}"),
    }
}

struct Builder<'a> {
    given: &'a [CapRef],
    info: Option<&'a ContextSequenceInfo>,
    participants: Vec<Participant>,
    messages: Vec<Message>,
    blocks: Vec<AltBlock>,
}

impl<'a> Builder<'a> {
    fn new(
        entry_label: String,
        given: &'a [CapRef],
        info: Option<&'a ContextSequenceInfo>,
    ) -> Self {
        Builder {
            given,
            info,
            participants: vec![Participant {
                id: ENTRY_ID,
                kind: ParticipantKind::Entry,
                name: entry_label,
                span: None,
            }],
            messages: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn finish(self) -> SequenceModel {
        SequenceModel {
            participants: self.participants,
            messages: self.messages,
            blocks: self.blocks,
        }
    }

    fn participant_id(&mut self, kind: ParticipantKind, name: &str, span: Span) -> u32 {
        if let Some(p) = self
            .participants
            .iter()
            .find(|p| p.kind == kind && p.name == name)
        {
            return p.id;
        }
        let id = self.participants.len() as u32;
        self.participants.push(Participant {
            id,
            kind,
            name: name.to_string(),
            span: Some(span),
        });
        id
    }

    /// Walk a block's statements, then its tail — the traversal spine every
    /// call site (top-level body, `if`/`match` branch) shares.
    fn walk_block(&mut self, block: &Block, current_block: Option<BlockCtx>, depth: u32) {
        for stmt in &block.statements {
            match stmt {
                Statement::EffectLet(l) => {
                    self.walk_value(&l.value, current_block, depth, Some(Arrow::Awaited))
                }
                Statement::Do(d) => {
                    self.walk_value(&d.value, current_block, depth, Some(Arrow::Awaited))
                }
                Statement::Send(s) => {
                    self.walk_value(&s.value, current_block, depth, Some(Arrow::FireAndForget))
                }
                Statement::Let(l) => self.walk_value(&l.value, current_block, depth, None),
                Statement::Expect(e) => self.walk_value(&e.value, current_block, depth, None),
                Statement::Assign(a) => self.walk_value(&a.value, current_block, depth, None),
            }
        }
        self.walk_value(&block.tail, current_block, depth, None);
    }

    /// Classify one expression reached directly under a statement (or a
    /// block's tail): a lifeline call (only possible when `arrow.is_some()`,
    /// since a pure `let`/tail cannot bind an `Effect[_]`), or `if`/`match`
    /// control flow to recurse into. Anything else — plain computation,
    /// constructors, nested calls buried in argument expressions — folds into
    /// the current activation with no further descent (Tier-1 scope: only
    /// handler-body-level control flow is diagrammed, not arbitrary
    /// expression-level branching).
    fn walk_value(
        &mut self,
        expr: &Expr,
        current_block: Option<BlockCtx>,
        depth: u32,
        arrow: Option<Arrow>,
    ) {
        let inner = peel_paren(expr);
        match &inner.kind {
            ExprKind::If {
                then_block,
                else_block,
                ..
            } => self.walk_if(inner.span, then_block, else_block, current_block, depth),
            ExprKind::Match { arms, .. } => self.walk_match(inner.span, arms, current_block, depth),
            ExprKind::Block(b) => self.walk_block(b, current_block, depth),
            ExprKind::Call { .. }
            | ExprKind::ConstructorCall { .. }
            | ExprKind::MethodCall { .. } => {
                if let Some(arrow) = arrow {
                    self.classify_call(inner, arrow, current_block);
                }
            }
            _ => {}
        }
    }

    fn walk_if(
        &mut self,
        span: Span,
        then_block: &Block,
        else_block: &Block,
        current_block: Option<BlockCtx>,
        depth: u32,
    ) {
        if depth >= MAX_BLOCK_DEPTH {
            self.push_collapsed(span, current_block);
            return;
        }
        let id = self.blocks.len() as u32;
        self.blocks.push(AltBlock {
            id,
            kind: AltKind::If,
            branches: Vec::new(),
            span,
            parent: current_block.map(|c| c.id),
            parent_branch: current_block.map(|c| c.branch),
        });
        let then_start = self.messages.len();
        self.walk_block(then_block, Some(BlockCtx { id, branch: 0 }), depth + 1);
        let then_ids = (then_start..self.messages.len()).collect();
        let else_start = self.messages.len();
        // An implicit (synthesised) `()` else has nothing to say for itself —
        // still walked (it may itself contain lifeline calls it just doesn't
        // gate a return with), but its branch label reflects the omission.
        self.walk_block(else_block, Some(BlockCtx { id, branch: 1 }), depth + 1);
        let else_ids = (else_start..self.messages.len()).collect();
        self.blocks[id as usize].branches = vec![
            Branch {
                label: "then".to_string(),
                message_ids: then_ids,
            },
            Branch {
                label: "else".to_string(),
                message_ids: else_ids,
            },
        ];
    }

    fn walk_match(
        &mut self,
        span: Span,
        arms: &[MatchArm],
        current_block: Option<BlockCtx>,
        depth: u32,
    ) {
        if depth >= MAX_BLOCK_DEPTH {
            self.push_collapsed(span, current_block);
            return;
        }
        let id = self.blocks.len() as u32;
        self.blocks.push(AltBlock {
            id,
            kind: AltKind::Match,
            branches: Vec::new(),
            span,
            parent: current_block.map(|c| c.id),
            parent_branch: current_block.map(|c| c.branch),
        });
        let mut branches = Vec::with_capacity(arms.len());
        for (arm_index, arm) in arms.iter().enumerate() {
            let start = self.messages.len();
            let branch_ctx = Some(BlockCtx {
                id,
                branch: arm_index as u32,
            });
            match &arm.body {
                MatchBody::Expr(e) => self.walk_value(e, branch_ctx, depth + 1, None),
                MatchBody::Block(b) => self.walk_block(b, branch_ctx, depth + 1),
            }
            branches.push(Branch {
                label: pattern_summary(&arm.pattern),
                message_ids: (start..self.messages.len()).collect(),
            });
        }
        self.blocks[id as usize].branches = branches;
    }

    fn push_collapsed(&mut self, span: Span, current_block: Option<BlockCtx>) {
        let id = self.blocks.len() as u32;
        self.blocks.push(AltBlock {
            id,
            kind: AltKind::Collapsed,
            branches: Vec::new(),
            span,
            parent: current_block.map(|c| c.id),
            parent_branch: current_block.map(|c| c.branch),
        });
    }

    fn classify_call(&mut self, expr: &Expr, arrow: Arrow, current_block: Option<BlockCtx>) {
        let Some((target, label)) = self.classify_target(expr) else {
            // Local computation (commons/context-local fn, plain method,
            // constructor) — folds into the entry activation, no message.
            return;
        };
        let block = current_block.map(|c| c.id);
        match arrow {
            Arrow::FireAndForget => {
                self.messages.push(Message {
                    from: ENTRY_ID,
                    to: target,
                    kind: MessageKind::Send,
                    label,
                    span: expr.span,
                    block,
                });
            }
            Arrow::Awaited => {
                self.messages.push(Message {
                    from: ENTRY_ID,
                    to: target,
                    kind: MessageKind::Call,
                    label,
                    span: expr.span,
                    block,
                });
                self.messages.push(Message {
                    from: target,
                    to: ENTRY_ID,
                    kind: MessageKind::Return,
                    label: String::new(),
                    span: expr.span,
                    block,
                });
            }
        }
    }

    /// Resolve a call expression's target lifeline, per Decision A:
    /// consumed Capability > Agent > consumed Context. Returns the
    /// participant id and a rendered call label, or `None` when the call is
    /// local computation.
    ///
    /// `TypeName.method(args)` (`Clock.now()`, a local capability op) and
    /// `Consumed.service(args)` (a cross-context call) are syntactically
    /// identical to an ordinary instance method call — the parser has no
    /// static-vs-instance distinction at the receiver, so *every* qualified
    /// call parses uniformly as `ExprKind::MethodCall` with an `Ident`
    /// receiver (`ExprKind::ConstructorCall` is unreachable from the parser
    /// today; the resolver/checker make the static-vs-instance call from
    /// context, which this classifier reimplements against `given`/`agents`/
    /// `cross_context` instead). `Agent(key).method(args)` is the one
    /// receiver shape that differs structurally: the receiver is itself an
    /// `ExprKind::Call` (the agent construction), not a bare `Ident`.
    fn classify_target(&mut self, expr: &Expr) -> Option<(u32, String)> {
        match &expr.kind {
            ExprKind::MethodCall {
                receiver,
                method,
                args,
                ..
            } => match &receiver.kind {
                ExprKind::Call { name, .. }
                    if self.info.is_some_and(|i| i.agents.contains_key(&name.name)) =>
                {
                    let id = self.participant_id(ParticipantKind::Agent, &name.name, name.span);
                    Some((id, call_label(&method.name, args)))
                }
                ExprKind::Ident(id) => self.classify_static(&id.name, &method.name, args, id.span),
                _ => None,
            },
            // Kept for exhaustiveness against a possible future parser
            // change; unreachable today (see the doc comment above).
            ExprKind::ConstructorCall {
                type_name,
                method,
                args,
            } => self.classify_static(&type_name.name, &method.name, args, type_name.span),
            _ => None,
        }
    }

    /// Classify a bare qualified call `Name.method(args)`: a local
    /// capability op when `Name` is in the handler's effective `given`,
    /// otherwise a cross-context call when `Name` resolves as a consumed
    /// context (or alias). Anything else (a static method on an ordinary
    /// type, a sum-type variant constructor) is local — `None`.
    fn classify_static(
        &mut self,
        name: &str,
        method: &str,
        args: &[Expr],
        span: Span,
    ) -> Option<(u32, String)> {
        if self.given.iter().any(|c| c.key() == name) {
            let id = self.participant_id(ParticipantKind::Capability, name, span);
            return Some((id, call_label(method, args)));
        }
        self.classify_cross_context(name, method, args, span)
    }

    fn classify_cross_context(
        &mut self,
        prefix: &str,
        method: &str,
        args: &[Expr],
        span: Span,
    ) -> Option<(u32, String)> {
        let ctx_name = self.info?.cross_context.resolve_prefix(prefix)?;
        let id = self.participant_id(ParticipantKind::Context, &ctx_name, span);
        Some((id, call_label(method, args)))
    }
}

fn call_label(method: &str, args: &[Expr]) -> String {
    let rendered: Vec<String> = args.iter().map(bynk_fmt::expr_to_string).collect();
    format!("{method}({})", rendered.join(", "))
}

fn peel_paren(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Paren(inner) => peel_paren(inner),
        _ => expr,
    }
}

/// A match arm's branch label — a short rendering of its pattern, since
/// there is no dedicated pattern-to-source printer to reuse.
fn pattern_summary(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard(_) => "_".to_string(),
        Pattern::Binding(b) => b.name.clone(),
        Pattern::Literal { value, .. } => value.describe(),
        Pattern::Variant {
            type_name, variant, ..
        } => match type_name {
            Some(t) => format!("{}.{}", t.name, variant.name),
            None => variant.name.clone(),
        },
        Pattern::Refined { inner, .. } => pattern_summary(inner),
        Pattern::Or(patterns, _) => patterns
            .iter()
            .map(pattern_summary)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;
    use std::fs;
    use std::path::PathBuf;

    /// Same convention as `symbols.rs`'s `setup_project`: a temp dir unique to
    /// the test name, populated with `(relative_path, contents)` files.
    /// Self-contained fixtures only (never `examples/`) — `bynk-ide` is
    /// published standalone, and a test reaching outside the crate would fail
    /// a `cargo test` on the released tarball.
    fn setup_project(test_name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bynk-ide-sequence-test-{test_name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        for (rel, contents) in files {
            let p = root.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&p, contents).expect("write file");
        }
        root
    }

    fn parse_context(text: &str) -> Context {
        let tokens = bynk_syntax::lexer::tokenize(text).expect("tokenize");
        let (unit, errs) = bynk_syntax::parser::parse_unit_with_recovery(&tokens, text);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        match unit.expect("parsed unit") {
            SourceUnit::Context(c) => c,
            _ => panic!("expected a context"),
        }
    }

    fn find_service<'a>(ctx: &'a Context, name: &str) -> &'a ServiceDecl {
        ctx.items
            .iter()
            .find_map(|i| match i {
                CommonsItem::Service(s) if s.name.name == name => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| panic!("service {name} not found"))
    }

    // -- Fixture 1: examples/rate-limiter's `GET /check/:client`, reproduced
    // -- self-contained (not read from `examples/` — see `setup_project`).
    // -- Capability (Clock) + Agent (Limiter) + a return-gating `if` whose
    // -- branches call nothing lifeline-worthy — the regression fixture for
    // -- the corrected AltBlock rule (see the plan's "Corrected extractor
    // -- rule" note): the issue's own worked example renders this block.
    const RATELIMIT_SRC: &str = r#"context ratelimit

consumes bynk { Clock }

type ClientId = String where NonEmpty

type RateView = {
  allowed:   Bool,
  remaining: Int,
  resetAt:   Int,
}

agent Limiter {
  key client: ClientId

  store count: Cell[Int]

  on call hit(now: Int) -> Effect[RateView] {
    let _ <- count.update((c) => c + 1)
    RateView { allowed: count < 10, remaining: 10 - count, resetAt: now }
  }
}

service api from http {
  on GET("/check/:client") (client: ClientId) -> Effect[HttpResult[RateView]] by Visitor given Clock {
    let now  <- Clock.now()
    let view <- Limiter(client).hit(now.toEpochMillis())
    if view.allowed {
      Ok(view)
    } else {
      TooManyRequests("rate limit exceeded")
    }
  }
}
"#;

    #[test]
    fn rate_limiter_get_check_client_classifies_capability_and_agent_and_gates_the_return() {
        let root = setup_project("ratelimit", &[("ratelimit.bynk", RATELIMIT_SRC)]);
        let diag = crate::diagnose_project(&root, &Map::new());
        let info = diag
            .sequence_info
            .get("ratelimit")
            .expect("sequence_info entry for ratelimit");

        let ctx = parse_context(RATELIMIT_SRC);
        let svc = find_service(&ctx, "api");
        let handler = &svc.handlers[0];

        let model = sequence_model(
            handler,
            HandlerOwner::Service("api"),
            &svc.default_given,
            Some(info),
        );

        let kinds: Vec<(ParticipantKind, &str)> = model
            .participants
            .iter()
            .map(|p| (p.kind, p.name.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (ParticipantKind::Entry, "api GET /check/:client"),
                (ParticipantKind::Capability, "Clock"),
                (ParticipantKind::Agent, "Limiter"),
            ]
        );
        assert_eq!(
            model.messages.len(),
            4,
            "Call+Return for Clock.now(), Call+Return for Limiter(client).hit(...)"
        );
        assert_eq!(
            model.blocks.len(),
            1,
            "the if/else gating the final return must produce an AltBlock even though \
             neither branch calls a lifeline — matches the issue's own worked example"
        );
        assert_eq!(model.blocks[0].kind, AltKind::If);
        assert_eq!(model.blocks[0].branches.len(), 2);
        assert!(
            model.blocks[0]
                .branches
                .iter()
                .all(|b| b.message_ids.is_empty()),
            "neither branch calls anything lifeline-worthy — only the block itself is the signal"
        );
    }

    // -- Fixture 2: a consumed-context call — boundary-stop (Decision C).
    const PLATFORM_SRC: &str = r#"context platform

service Pinger {
  on call(n: Int) -> Effect[Int] {
    n
  }
}
"#;
    const CONSUMER_SRC: &str = r#"context consumer

consumes platform

service api {
  on call(n: Int) -> Effect[Int] {
    let v <- platform.Pinger(n)
    v
  }
}
"#;

    #[test]
    fn cross_context_call_is_boundary_stop() {
        let root = setup_project(
            "crossctx",
            &[
                ("platform.bynk", PLATFORM_SRC),
                ("consumer.bynk", CONSUMER_SRC),
            ],
        );
        let diag = crate::diagnose_project(&root, &Map::new());
        let info = diag
            .sequence_info
            .get("consumer")
            .expect("sequence_info entry for consumer");

        let ctx = parse_context(CONSUMER_SRC);
        let svc = find_service(&ctx, "api");
        let handler = &svc.handlers[0];
        let model = sequence_model(
            handler,
            HandlerOwner::Service("api"),
            &svc.default_given,
            Some(info),
        );

        assert_eq!(model.participants.len(), 2, "Entry + the consumed context");
        assert_eq!(model.participants[1].kind, ParticipantKind::Context);
        assert_eq!(model.participants[1].name, "platform");
        assert_eq!(
            model.messages.len(),
            2,
            "one Call + one Return — the consumed service's own body is never walked"
        );
        assert_eq!(model.messages[0].kind, MessageKind::Call);
        assert_eq!(model.messages[1].kind, MessageKind::Return);
    }

    // -- Fixtures 3-5: fire-and-forget send, a degenerate (local-only)
    // -- handler, and 3-level-nested `if` past the depth budget.
    const MISC_SRC: &str = r#"context misc

consumes bynk { Clock, Logger }

fn double(x: Int) -> Int {
  x * 2
}

service fireService {
  on call(n: Int) -> Effect[()] given Logger {
    ~> Logger.info("hi")
    Effect.pure(())
  }
}

service localService {
  on call(n: Int) -> Effect[Int] {
    double(n)
  }
}

service nestedService {
  on call(n: Int) -> Effect[Int] given Clock {
    let now <- Clock.now()
    if n > 0 {
      if n > 10 {
        if n > 100 {
          now.toEpochMillis()
        } else {
          1
        }
      } else {
        2
      }
    } else {
      3
    }
  }
}
"#;

    fn misc_info(diag: &crate::ProjectDiagnostics) -> bynk_emit::project::ContextSequenceInfo {
        diag.sequence_info
            .get("misc")
            .cloned()
            .expect("sequence_info entry for misc")
    }

    #[test]
    fn fire_and_forget_send_has_no_paired_return() {
        let root = setup_project("misc-send", &[("misc.bynk", MISC_SRC)]);
        let diag = crate::diagnose_project(&root, &Map::new());
        let info = misc_info(&diag);

        let ctx = parse_context(MISC_SRC);
        let svc = find_service(&ctx, "fireService");
        let handler = &svc.handlers[0];
        let model = sequence_model(
            handler,
            HandlerOwner::Service("fireService"),
            &svc.default_given,
            Some(&info),
        );

        assert_eq!(model.participants.len(), 2);
        assert_eq!(model.participants[1].kind, ParticipantKind::Capability);
        assert_eq!(model.participants[1].name, "Logger");
        assert_eq!(model.messages.len(), 1, "a Send has no paired Return");
        assert_eq!(model.messages[0].kind, MessageKind::Send);
    }

    #[test]
    fn degenerate_handler_with_only_local_calls_has_no_lifelines() {
        let root = setup_project("misc-local", &[("misc.bynk", MISC_SRC)]);
        let diag = crate::diagnose_project(&root, &Map::new());
        let info = misc_info(&diag);

        let ctx = parse_context(MISC_SRC);
        let svc = find_service(&ctx, "localService");
        let handler = &svc.handlers[0];
        let model = sequence_model(
            handler,
            HandlerOwner::Service("localService"),
            &svc.default_given,
            Some(&info),
        );

        assert_eq!(model.participants.len(), 1);
        assert_eq!(model.participants[0].kind, ParticipantKind::Entry);
        assert!(model.messages.is_empty());
        assert!(model.blocks.is_empty());
    }

    #[test]
    fn nested_if_collapses_past_the_depth_budget() {
        let root = setup_project("misc-nested", &[("misc.bynk", MISC_SRC)]);
        let diag = crate::diagnose_project(&root, &Map::new());
        let info = misc_info(&diag);

        let ctx = parse_context(MISC_SRC);
        let svc = find_service(&ctx, "nestedService");
        let handler = &svc.handlers[0];
        let model = sequence_model(
            handler,
            HandlerOwner::Service("nestedService"),
            &svc.default_given,
            Some(&info),
        );

        // Depth 0 (`n > 0`) and depth 1 (`n > 10`) render as real blocks;
        // depth 2 (`n > 100`) is past `MAX_BLOCK_DEPTH` and collapses instead
        // of expanding further — click-to-code still works (the collapsed
        // marker's span points at the whole collapsed `if`), the diagram
        // just doesn't recurse into it.
        let kinds: Vec<AltKind> = model.blocks.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, vec![AltKind::If, AltKind::If, AltKind::Collapsed]);
        assert!(
            model.blocks[2].branches.is_empty(),
            "a Collapsed block carries no branches"
        );
        // Regression: each nested block must record which branch of its
        // parent it sits under, not just the parent id — both the middle
        // and innermost `if` are nested in their parent's *first* ("then")
        // branch, and a renderer needs that even when the branch itself
        // carries no messages of its own (only the nested block does).
        assert_eq!(model.blocks[0].parent, None);
        assert_eq!(model.blocks[0].parent_branch, None);
        assert_eq!(model.blocks[1].parent, Some(0));
        assert_eq!(model.blocks[1].parent_branch, Some(0));
        assert_eq!(model.blocks[2].parent, Some(1));
        assert_eq!(model.blocks[2].parent_branch, Some(0));
    }

    // -- Fixture 6 (#861 review): a service declares a service-level `given`
    // -- default (v0.155) and the handler omits its own `given`, inheriting it.
    // -- The classifier walks a freshly-parsed AST that never ran
    // -- `inject_service_defaults`, so it must apply the fallback itself — or
    // -- the Clock capability lifeline is silently dropped.
    const SERVICE_GIVEN_SRC: &str = r#"context svcgiven

consumes bynk { Clock }

service api from http by Visitor given Clock {
  on GET("/now") () -> Effect[HttpResult[Int]] {
    let now <- Clock.now()
    Ok(now.toEpochMillis())
  }
}
"#;

    #[test]
    fn service_level_given_default_is_inherited_by_a_handler_without_its_own() {
        let root = setup_project("svcgiven", &[("svcgiven.bynk", SERVICE_GIVEN_SRC)]);
        let diag = crate::diagnose_project(&root, &Map::new());
        let info = diag
            .sequence_info
            .get("svcgiven")
            .expect("sequence_info entry for svcgiven");

        let ctx = parse_context(SERVICE_GIVEN_SRC);
        let svc = find_service(&ctx, "api");
        let handler = &svc.handlers[0];
        // Precondition: the freshly-parsed handler carries no `given` of its
        // own — it relies entirely on the service-level default.
        assert!(
            handler.given.is_empty(),
            "fixture handler must omit its own `given`"
        );
        assert_eq!(svc.default_given.len(), 1, "service-level `given Clock`");

        let model = sequence_model(
            handler,
            HandlerOwner::Service("api"),
            &svc.default_given,
            Some(info),
        );

        let kinds: Vec<(ParticipantKind, &str)> = model
            .participants
            .iter()
            .map(|p| (p.kind, p.name.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (ParticipantKind::Entry, "api GET /now"),
                (ParticipantKind::Capability, "Clock"),
            ],
            "Clock must classify as a capability lifeline via the inherited \
             service-level `given` — with only `handler.given` it would be dropped"
        );
    }
}
