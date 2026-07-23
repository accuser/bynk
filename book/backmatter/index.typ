#import "../template.typ": apparatus-note, subject-index

#let index-proof = (
  (
    letter: "A",
    entries: (
      (
        term: [actors],
        refs: (
          <who-is-calling-is-part-of-the-operation>,
          <reading-a-whole-system>,
        ),
      ),
      (
        term: [adapters],
        refs: (
          <a-new-language-should-not-require-a-new-universe>,
          <the-cost-of-stronger-constraints>,
        ),
      ),
      (
        term: [admission],
        refs: (
          <a-data-shape-is-not-a-domain-model>,
          <failure-is-part-of-the-contract>,
        ),
      ),
      (
        term: [agents],
        refs: (
          <state-needs-an-owner>,
          <state-changes-are-contracts>,
          <reading-a-whole-system>,
        ),
      ),
      (
        term: [architecture],
        refs: (<prologue>, <epilogue>),
        subs: (
          (
            term: [as convention],
            refs: (<when-architecture-becomes-convention>,),
          ),
          (
            term: [compiler-visible],
            refs: (
              <a-compiler-refusal-can-teach-the-design>,
              <epilogue>,
            ),
          ),
          (
            term: [recovering from source],
            refs: (<reading-a-whole-system>,),
          ),
        ),
      ),
      (
        term: [authentication],
        refs: (
          <who-is-calling-is-part-of-the-operation>,
          <reading-a-whole-system>,
        ),
      ),
      (
        term: [authorisation],
        refs: (
          <who-is-calling-is-part-of-the-operation>,
          <reading-a-whole-system>,
        ),
      ),
    ),
  ),
  (
    letter: "B",
    entries: (
      (
        term: [boundaries],
        refs: (
          <when-architecture-becomes-convention>,
          <time-and-messages-are-architectural-boundaries>,
        ),
        subs: (
          (
            term: [entry points],
            refs: (<time-and-messages-are-architectural-boundaries>,),
          ),
          (
            term: [host language],
            refs: (
              <a-new-language-should-not-require-a-new-universe>,
              <the-cost-of-stronger-constraints>,
            ),
          ),
        ),
      ),
      (
        term: [Bynk],
        refs: (<prologue>, <epilogue>),
        subs: (
          (
            term: [costs and fit],
            refs: (<the-cost-of-stronger-constraints>,),
          ),
          (
            term: [relationship to TypeScript],
            refs: (<a-new-language-should-not-require-a-new-universe>,),
          ),
        ),
      ),
    ),
  ),
  (
    letter: "C",
    entries: (
      (
        term: [capabilities],
        refs: (
          <effects-should-name-their-requirements>,
          <tests-should-preserve-the-architecture>,
        ),
      ),
      (
        term: [caller identity],
        see: [actors],
      ),
      (
        term: [Cloudflare Workers],
        refs: (
          <a-new-language-should-not-require-a-new-universe>,
          <reading-a-whole-system>,
        ),
      ),
      (
        term: [compiler diagnostics],
        refs: (<a-compiler-refusal-can-teach-the-design>,),
      ),
      (
        term: [compiler refusals],
        refs: (
          <a-compiler-refusal-can-teach-the-design>,
          <epilogue>,
        ),
      ),
      (
        term: [compensation],
        refs: (
          <reading-a-whole-system>,
          <the-cost-of-stronger-constraints>,
        ),
      ),
      (
        term: [constraints],
        refs: (<the-cost-of-stronger-constraints>,),
      ),
      (
        term: [contexts],
        refs: (
          <when-architecture-becomes-convention>,
          <reading-a-whole-system>,
        ),
      ),
    ),
  ),
  (
    letter: "D",
    entries: (
      (
        term: [dependency graphs],
        refs: (
          <when-architecture-becomes-convention>,
          <a-compiler-refusal-can-teach-the-design>,
          <reading-a-whole-system>,
        ),
      ),
      (
        term: [deployment topology],
        refs: (
          <a-new-language-should-not-require-a-new-universe>,
          <reading-a-whole-system>,
        ),
      ),
      (
        term: [domain models],
        refs: (<a-data-shape-is-not-a-domain-model>,),
      ),
    ),
  ),
  (
    letter: "E",
    entries: (
      (
        term: [effects],
        refs: (
          <effects-should-name-their-requirements>,
          <tests-should-preserve-the-architecture>,
        ),
      ),
      (
        term: [entry points],
        refs: (<time-and-messages-are-architectural-boundaries>,),
      ),
      (
        term: [exhaustiveness],
        refs: (
          <failure-is-part-of-the-contract>,
          <a-compiler-refusal-can-teach-the-design>,
        ),
      ),
    ),
  ),
  (
    letter: "F",
    entries: (
      (
        term: [failure contracts],
        refs: (
          <failure-is-part-of-the-contract>,
          <reading-a-whole-system>,
        ),
      ),
    ),
  ),
  (
    letter: "H",
    new-column: true,
    entries: (
      (
        term: [histories],
        refs: (<tests-should-preserve-the-architecture>,),
      ),
      (
        term: [HTTP],
        refs: (
          <time-and-messages-are-architectural-boundaries>,
          <reading-a-whole-system>,
        ),
      ),
    ),
  ),
  (
    letter: "I",
    entries: (
      (
        term: [identity],
        refs: (
          <a-data-shape-is-not-a-domain-model>,
          <who-is-calling-is-part-of-the-operation>,
        ),
      ),
      (
        term: [invariants],
        refs: (
          <state-needs-an-owner>,
          <state-changes-are-contracts>,
        ),
      ),
    ),
  ),
  (
    letter: "M",
    entries: (
      (
        term: [messages],
        refs: (<time-and-messages-are-architectural-boundaries>,),
      ),
    ),
  ),
  (
    letter: "O",
    entries: (
      (
        term: [opaque values],
        refs: (<a-data-shape-is-not-a-domain-model>,),
      ),
      (
        term: [ownership],
        refs: (
          <state-needs-an-owner>,
          <reading-a-whole-system>,
        ),
      ),
    ),
  ),
  (
    letter: "P",
    entries: (
      (
        term: [plugins],
        refs: (<the-cost-of-stronger-constraints>,),
      ),
      (
        term: [providers],
        refs: (<effects-should-name-their-requirements>,),
      ),
    ),
  ),
  (
    letter: "Q",
    entries: (
      (
        term: [queues],
        refs: (<time-and-messages-are-architectural-boundaries>,),
      ),
    ),
  ),
  (
    letter: "R",
    entries: (
      (
        term: [recoverability],
        refs: (
          <reading-a-whole-system>,
          <the-cost-of-stronger-constraints>,
        ),
      ),
      (
        term: [refined values],
        refs: (<a-data-shape-is-not-a-domain-model>,),
      ),
      (
        term: [`Result`],
        refs: (<failure-is-part-of-the-contract>,),
      ),
      (
        term: [retries],
        refs: (<time-and-messages-are-architectural-boundaries>,),
      ),
    ),
  ),
  (
    letter: "S",
    entries: (
      (
        term: [schedules],
        refs: (<time-and-messages-are-architectural-boundaries>,),
      ),
      (
        term: [state transitions],
        refs: (<state-changes-are-contracts>,),
      ),
      (
        term: [stubs],
        refs: (<tests-should-preserve-the-architecture>,),
      ),
    ),
  ),
  (
    letter: "T",
    entries: (
      (
        term: [test tiers],
        refs: (<tests-should-preserve-the-architecture>,),
      ),
      (
        term: [testing],
        refs: (<tests-should-preserve-the-architecture>,),
      ),
      (
        term: [transactions],
        refs: (
          <reading-a-whole-system>,
          <the-cost-of-stronger-constraints>,
        ),
      ),
      (
        term: [TypeScript],
        refs: (<a-new-language-should-not-require-a-new-universe>,),
        subs: (
          (
            term: [emission],
            refs: (<a-new-language-should-not-require-a-new-universe>,),
          ),
          (
            term: [when it fits better],
            refs: (<the-cost-of-stronger-constraints>,),
          ),
        ),
      ),
    ),
  ),
  (
    letter: "V",
    entries: (
      (
        term: [validation],
        see: [admission],
      ),
    ),
  ),
  (
    letter: "W",
    entries: (
      (
        term: [WebSockets],
        refs: (<time-and-messages-are-architectural-boundaries>,),
      ),
      (
        term: [Workers],
        see: [Cloudflare Workers],
      ),
    ),
  ),
)

= Index <index>

#apparatus-note[
  Editorial proof: coverage and locators are provisional. The final index will
  be marked at significant discussions during the revision pass.
]

#subject-index(index-proof)
