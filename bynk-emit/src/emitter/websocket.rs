//! v0.104 (real-time track slice 3b): shared analysis of a `from WebSocket`
//! `on open` handler for the Workers wire path.
//!
//! On Workers the upgrade is authenticated at the edge, then forwarded to the
//! **Durable Object that hosts the connection** — the agent the `on open`
//! transfers it to. For that routing to be static, the handler must transfer the
//! connection to exactly one agent, by a key derivable from the request (D2).
//! This module finds that single transfer; both the checker (to diagnose a bad
//! shape) and the emitter (to route the upgrade) use it.

use std::collections::HashSet;

use bynk_syntax::ast::{Block, Expr, ExprKind, Statement};

/// The single connection-transfer target a `from WebSocket` `on open` resolves
/// to: the agent that will host the connection, and the key expression
/// addressing the instance (`Room(roomId)` → agent `Room`, key `roomId`).
pub(crate) struct WsOpenTarget<'a> {
    pub agent: &'a str,
    pub key: &'a Expr,
}

/// The shape of an `on open` body's connection handling (D2).
pub(crate) enum WsOpenShape<'a> {
    /// Exactly one top-level agent transfer — the routable case.
    One(WsOpenTarget<'a>),
    /// No top-level agent transfer (e.g. the connection is only closed, or
    /// transferred inside a conditional — not statically routable).
    None,
    /// More than one agent transfer — ambiguous routing.
    Multiple,
}

/// The synthetic name of the held connection an `on open` handler receives.
pub(crate) const CONNECTION_BINDING: &str = "connection";

/// Analyse a `from WebSocket` `on open` body: find the **top-level** agent
/// transfers of the `connection` binding (a `let _ <- Agent(key).m(…, connection)`
/// statement). A transfer nested in a conditional is deliberately *not* counted —
/// the host DO must be statically resolvable.
pub(crate) fn analyse_open_shape<'a>(
    body: &'a Block,
    local_agents: &HashSet<String>,
) -> WsOpenShape<'a> {
    let mut targets: Vec<WsOpenTarget<'a>> = Vec::new();
    for stmt in &body.statements {
        let value = match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => &l.value,
            _ => continue,
        };
        if let Some(t) = transfer_target(value, local_agents) {
            targets.push(t);
        }
    }
    // The tail is an expression too (rare, but a transfer could be the tail).
    if let Some(t) = transfer_target(&body.tail, local_agents) {
        targets.push(t);
    }
    match targets.len() {
        0 => WsOpenShape::None,
        1 => WsOpenShape::One(targets.pop().unwrap()),
        _ => WsOpenShape::Multiple,
    }
}

/// If `e` is `Agent(key).method(… connection …)` for a known agent and the
/// `connection` binding is one of the call arguments, return the (agent, key).
fn transfer_target<'a>(e: &'a Expr, local_agents: &HashSet<String>) -> Option<WsOpenTarget<'a>> {
    let ExprKind::MethodCall { receiver, args, .. } = &e.kind else {
        return None;
    };
    let ExprKind::Call {
        name,
        args: ctor_args,
        ..
    } = &receiver.kind
    else {
        return None;
    };
    if !local_agents.contains(&name.name) {
        return None;
    }
    let transfers_connection = args
        .iter()
        .any(|a| matches!(&a.kind, ExprKind::Ident(id) if id.name == CONNECTION_BINDING));
    if !transfers_connection {
        return None;
    }
    let key = ctor_args.first()?;
    Some(WsOpenTarget {
        agent: name.name.as_str(),
        key,
    })
}
