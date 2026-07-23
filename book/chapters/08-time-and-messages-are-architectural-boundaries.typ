#import "../template.typ": code-listing, compiler-message

#let source-lines(path, start, end) = {
  read(path).split("\n").slice(start, end).join("\n")
}

= Time and messages are architectural boundaries <time-and-messages-are-architectural-boundaries>

A payment requested over HTTP, a payment retry taken from a queue, and a
reconciliation started by a schedule may eventually call the same domain
operation. That does not make them the same operation.

In the first case, somebody is waiting for a response. In the second, a
delivery system needs a verdict about the message. In the third, no caller is
waiting and there may be no retry before the next scheduled run. A message on a
WebSocket adds a fourth shape: it belongs to a connection whose lifetime extends
beyond any one handler invocation.

These differences determine what success means, who may try again, which time
the work belongs to, and what must remain alive when a handler returns. They
are easy to lose because implementation languages give all four boundaries the
same convenient shape: an asynchronous callback.

== A callback can hide the agency

A team might deliberately standardise its entry points like this:

#code-listing(
  [One callback type makes four entry mechanisms look interchangeable],
  read("../snippets/chapter-08/conventional.ts"),
  lang: "typescript",
)

The abstraction has an attractive property. Delivery logic is written once,
and each adapter registers the same function. Test doubles can invoke it
without an HTTP server, queue, scheduler, or socket.

The common type also discards every protocol decision. `DeliveryResult` does not
say whether a temporary failure should become HTTP 503, a queue retry, a logged
cron failure, or a frame sent to a connected client. It does not say who is
waiting for the promise. It cannot express a scheduled instant or the ownership
of a connection.

The adapter implementations can restore those meanings. Good adapters do.
But the shared `EntryPoint` contract does not require them to, and a reviewer
at the registration site cannot tell what each result will cause.

Reusing the domain operation is sound. Reusing the boundary contract is the
mistake.

== Let the protocol own the verdict

The Bynk version keeps one effectful delivery requirement:

#code-listing(
  [The reusable operation knows nothing about how work arrived],
  source-lines(
    "../snippets/chapter-08/declared/src/commerce/notifications/delivery.bynk",
    0,
    17,
  ),
  lang: "bynk",
)

`Mailer.send` returns the domain outcome that every entry point needs:
successful delivery, a temporary failure, or a permanent failure. It does not
decide what the surrounding protocol should do with that outcome.

The HTTP and queue handlers make different translations:

#code-listing(
  [The same delivery outcome has two different boundary meanings],
  source-lines(
    "../snippets/chapter-08/declared/src/commerce/notifications/delivery.bynk",
    18,
    39,
  ),
  lang: "bynk",
)

The HTTP caller waits for one response. Successful delivery becomes
`NoContent`. A temporary mailer failure becomes `ServiceUnavailable`, and a
permanent recipient problem becomes `UnprocessableEntity`. Those statuses
inform the caller; they do not cause the Bynk runtime to repeat the request. The
remote caller owns any subsequent retry policy.

The queue has a different principal to answer: the delivery system. `Ack` says
that the message is finished and may be removed. `Retry(reason)` asks for
redelivery. A temporary mailer failure therefore becomes `Retry`, while a
permanent failure becomes `Ack` even though the email was not sent. Repeating a
poison message cannot make its recipient valid.

This separation is more precise than treating `Ok` as acknowledgement and
`Err` as retry. A queue consumer may need to acknowledge a domain failure, as
this one does. It may also need to retry after some effects have already
succeeded. The delivery verdict and the domain result answer different
questions.

That second case carries a warning. If the mailer accepts the email and the
handler fails before its message is acknowledged, redelivery may send the email
again. `QueueResult` makes the retry decision visible; it does not make the
operation idempotent. The message needs a stable identity, and the owner of the
effect may need to remember that identity, if duplicates are unacceptable.
Dead-letter policy also remains queue configuration outside this handler.

== Scheduled time is not the current time

A schedule has neither a request caller nor a message to acknowledge:

#code-listing(
  [A scheduled run receives its intended instant and reports a logged result],
  source-lines(
    "../snippets/chapter-08/declared/src/commerce/notifications/delivery.bynk",
    40,
    53,
  ),
  lang: "bynk",
)

The expression `0 8 * * *` declares one daily run at hour eight. Its `at`
parameter is the scheduled fire time in Unix epoch milliseconds. It is not a
call to a clock made after the handler starts. If a run begins late, the
scheduled instant still identifies the time bucket it was meant to process.

That distinction matters for daily summaries, reconciliation windows, and
idempotency keys. “Process the 08:00 run” is a stable instruction. “Process
whatever day `now` happens to report after startup delay” is not.

The return type is `Result[(), String]`. `Ok(())` completes silently.
`Err(...)` is logged and the run completes. Cron has no retry channel: a failure
does not secretly turn this invocation into another one. The next scheduled
fire is a new event, and catch-up or retry behaviour must be designed
explicitly.

The example includes `at` in the digest subject only to keep the relationship
visible. A production system would usually turn it into a domain time or
period type before using it. An epoch integer is precise transport information,
not yet a rich model of business time.

== A useful refusal

The protocol distinction becomes clearest when a queue handler returns an
ordinary domain result:

#code-listing(
  [This result says whether work succeeded, but not what to do with the message],
  read("../snippets/chapter-08/wrong-verdict/src/commerce/notifications.bynk"),
  lang: "bynk",
)

The compiler refuses the handler:

#compiler-message[
[bynk.queue.return_not_queue_result] Error:
`on message` handler must return `Effect[QueueResult]`,
but got `Effect[Result[(), SendError]]`
]

`Ok(())` sounds successful, but it leaves the queue's agency unstated. Should
the message be removed? Is a logical failure permanent? Did partial success
make retry dangerous? A queue handler must answer with `Ack` or `Retry` because
the infrastructure, not a synchronous caller, acts on its answer.

The compiler does not choose the verdict. It makes the missing choice visible.

== A connection is not a request

HTTP, queue, and cron handlers can release their input when they return. A
WebSocket opening creates a resource whose reason for existing is to outlive
that first handler.

The tracking service declares both directions of the conversation and all
three lifecycle events:

#code-listing(
  [Opening, receiving, and closing are separate parts of one connection],
  source-lines(
    "../snippets/chapter-08/declared/src/commerce/tracking/gateway.bynk",
    13,
    32,
  ),
  lang: "bynk",
)

`ClientFrame` is the shape the client may send; `ServerFrame` is the shape the
server may send. The upgrade authenticates `Subscriber` before the connection
is accepted. On success, `on open` receives an owned
`Connection[ServerFrame]`.

Sending the initial frame does not consume that connection. Transferring it to
`Tracking(trackingId).join(...)` does. The opening handler may then return
because another owner is responsible for the live resource:

#code-listing(
  [The keyed agent holds the connection until the close event removes it],
  source-lines(
    "../snippets/chapter-08/declared/src/commerce/tracking/gateway.bynk",
    33,
    52,
  ),
  lang: "bynk",
)

The `on message` and `on close` handlers recover the same route identity and
delegate to that agent. Removing the stored connection closes it. On the
Workers target, the connection can survive hibernation and be restored with the
agent.

This is stronger than adding a `socket` property to a generic event object. The
connection is a held resource. It must move into exactly one routable agent,
cannot be used after transfer, and cannot be forgotten when a branch returns.
The compiler is checking lifetime and ownership because those are part of the
WebSocket boundary.

The model is intentionally particular. A WebSocket service has one opening
shape, typed inbound and outbound frames, and an owner for each accepted
connection. Applications that need arbitrary upgrade routing or a library's
unrestricted socket object may find the constraint too narrow. The gain is that
long-lived state does not become an invisible exception to the ownership model
from Chapter 5.

== Four boundaries, four promises

The contrasts can be summarised without collapsing them:

#figure(
  block(width: 100%)[
    #set text(size: 8.2pt, hyphenate: false)
    #set par(justify: false, leading: 0.56em, first-line-indent: 0pt)
    #table(
      columns: (0.62fr, 0.92fr, 1.15fr, 1.25fr),
      inset: (x: 0.45em, y: 0.48em),
      stroke: (x, y) => if y == 1 { (bottom: 0.8pt + rgb("#4b44d6")) } else { none },
      table.header(
        text(weight: "semibold")[Boundary],
        text(weight: "semibold")[Begins with],
        text(weight: "semibold")[Handler determines],
        text(weight: "semibold")[Time or lifetime],
      ),
      [HTTP], [A request], [The response to its caller], [One request-response exchange],
      [Queue], [A delivered message], [Acknowledgement or redelivery], [May repeat; duplicates matter],
      [Cron], [A schedule firing], [Logged success or failure], [The declared scheduled instant],
      [WebSocket], [An authenticated upgrade], [The connection's lifecycle effect], [A connection outlives handlers],
    )
  ],
  caption: [A shared domain operation does not imply a shared boundary contract.],
)

The handler forms and return types are not ceremony around the same callback.
They identify who has agency after the handler finishes. An HTTP result gives
the remote caller information. A queue result instructs the broker. A cron
result records the run. A WebSocket lifecycle handler changes the state of a
continuing conversation.

There are further protocol details: path admission and status codes, malformed
message handling, schedule validation, frame authentication, connection
hibernation. Those belong in the online reference. The architectural point is
smaller and more durable: a boundary should retain the guarantees of the
mechanism that crossed it.

== Could TypeScript do this?

Yes. Mature TypeScript systems use different adapter interfaces for HTTP,
queues, schedules, and WebSockets. Queue libraries expose acknowledgement and
retry. Schedulers provide a fire time. WebSocket frameworks expose connection
lifecycle. Branded types and lint rules can keep the adapters from collapsing
into one generic callback.

That can be the right design, especially when platform choice or protocol
details change frequently. Bynk's closed set of entry protocols is a cost. A
new transport cannot be introduced as an ordinary library interface; the
language, compiler, and runtime must agree on its semantics. Even within the
supported set, deployment policies such as dead-letter configuration remain
outside the program.

Bynk's wager is that these four boundaries are common and consequential enough
to deserve language support. Their source forms preserve the questions that a
generic callback loses: who waits, who retries, which time applies, and who owns
what survives.

That completes the argument of Part II. An effect names what work requires. An
agent names who owns state. A state contract names what may be committed. An
actor names who crosses a boundary. An entry protocol names the temporal and
delivery rules under which the call occurs.

Together they make more of the architecture visible in the program. They do
not prove that the architecture behaves as intended.

A queue handler can choose the wrong verdict. An invariant can be too weak. A
capability provider can return an inconvenient answer. A WebSocket
conversation can produce an unexpected history while every individual call is
well typed.

Part III turns to confidence: how to test these declared boundaries without
constructing a second, easier architecture that exists only in the test suite.
