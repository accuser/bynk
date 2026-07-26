---
level: patch
changelog: The checker now rejects direct construction of a `uses`-sourced commons sum type's variant (bare or qualified constructor call) inside a context, where the per-context rebrand leaves that constructor out of value scope — closing the enforcement gap ADR 0256 documented, so `bynkc check` catches what would otherwise be a clean check followed by a `tsc` failure
---
