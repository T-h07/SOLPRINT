# Roadmap UI Integration

UI quality is a **cross-cutting requirement** of the SOLPRINT roadmap, not a Phase 10
concern. This document records the additions each phase carries. See
[UI_UX_SPEC.md](UI_UX_SPEC.md) for the underlying specification.

| Phase | UI obligation |
|---|---|
| 00 — Foundation | design system, tokens, primitives, a11y baseline, visual QA harness |
| 03–09 — Backend phases | expose UI-facing state deliberately |
| 10 — TERMINAL | workspace shell and all core views, passing every quality gate |
| 11 — SNIPER | execution UI built on existing safety primitives |
| 12–13 — Hardening | frontend performance and resilience testing |
| 14 — Release | final visual audit and full baseline coverage |

---

## Phase 00 — Foundation

**Adds:** design principles, theme tokens, typography rules, spacing/radius systems,
semantic colors, chart tokens, financial-number formatting, accessibility baseline,
foundational components, density variants, component showcase, screenshot fixtures and
visual-regression infrastructure. shadcn/ui's semantic tokens are replaced so the
React/Tailwind/shadcn stack coexists with SOLPRINT's identity.

**Completion condition:**

> SOLPRINT boots reproducibly with backend/frontend infrastructure operational, and the
> frontend has an approved black terminal design system, accessible foundational
> primitives, representative component states and repeatable visual QA.

## Phases 03–09 — Backend phases

Expose UI-facing state deliberately, even before TERMINAL exists:

- source timestamps;
- reason codes;
- data quality;
- stale state;
- error state (distinguishing provider failure, server failure, absent data);
- deterministic IDs;
- explicit Paper / Replay distinctions.

This prevents Phase 10 from having to reverse-engineer whether backend data is safe to
show.

## Phase 10 — TERMINAL

A design and workflow implementation phase, not "build dashboard pages."

**Deliverables:** workspace shell, portfolio hierarchy, wallet intelligence table,
Cortex inspector, Filter evidence, positions, Replay/Sandbox, provider/API monitoring,
persistent safety state, command navigation, responsive behavior and all required
component states.

**Completion condition:**

> Every major SOLPRINT workflow can be monitored and operated through a visually
> coherent professional terminal that passes functional, accessibility,
> screenshot-regression and human visual-review gates. No frontend task is considered
> complete merely because the data renders.

## Phase 11 — SNIPER

Reuses the safety primitives defined earlier. Each of these requires an explicit visual
specification before implementation:

- live enablement;
- transaction simulation;
- order review and confirmation;
- pending / confirmed / failed / uncertain transaction states;
- emergency stop.

Execution UI must not appear for the first time in this phase as generic buttons
assembled ad hoc.

## Phases 12–13 — Hardening

Frontend performance and resilience testing:

- high-frequency updates cause no visible layout shift;
- rapidly changing values do not re-render unrelated regions;
- lost provider connectivity visibly transitions to stale/degraded state;
- application restart restores mode and portfolio presentation without suggesting
  uncertain data is current.

## Phase 14 — Release

Final visual audit alongside regression testing. Approved screenshots for:

- all primary workflows;
- all breakpoints;
- Paper and Live states;
- error / stale / degraded states;
- realistic dense datasets.
