# Working outline

The structure is provisional. Chapter titles name the engineering problem; Bynk
constructs belong inside the argument rather than in the table of contents.

## Prologue: The diagram is already wrong

A small service accumulates callers, state, retries, scheduled work, and hidden
rules. Its diagram remains simple while its architecture becomes implicit.

## Part I - Saying what the system means

1. **When architecture becomes convention**
   How boundaries disappear into folders, imports, configuration, and memory.
2. **A data shape is not a domain model**
   Identity, validity, refined values, opaque values, and admission.
3. **Failure is part of the contract**
   Exceptions and absence versus explicit results and exhaustive choices.

## Part II - Ownership, effects, and authority

4. **Effects should name their requirements**
   Invisible effects, capabilities, providers, and constrained composition.
5. **State needs an owner**
   Identity, persistence, zeroable state, and the agent model.
6. **State changes are contracts**
   Invariants, transitions, and state-machine thinking.
7. **Who is calling is part of the operation**
   Authentication, authorisation, actors, and cross-context identity.
8. **Time and messages are architectural boundaries**
   HTTP, queues, cron, and WebSockets as distinct entry points.

## Part III - Confidence without illusion

9. **Tests should preserve the architecture**
   Test doubles, tiers, observation, histories, and false confidence.
10. **A compiler refusal can teach the design**
    Diagnostics as explanations of violated invariants.

## Part IV - The argument tested

11. **A new language should not require a new universe**
    Typed TypeScript, Cloudflare Workers, pragmatism, and compromise.
12. **Reading a whole system**
    Recovering boundaries, ownership, effects, callers, and failure from source.
13. **The cost of stronger constraints**
    Lost flexibility, unsolved problems, and when another language fits better.

## Epilogue: The program should not be able to forget

Return to the opening system and ask which parts of its architecture can now be
stated, checked, and preserved.
