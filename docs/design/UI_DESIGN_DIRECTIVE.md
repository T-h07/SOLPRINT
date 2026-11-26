# SOLPRINT UI Design Directive

Insert this directive, verbatim, into every prompt that creates or materially changes
frontend UI — not only Phase 10. The full rationale lives in
[UI_UX_SPEC.md](UI_UX_SPEC.md).

---

```text
SOLPRINT UI DESIGN DIRECTIVE — MANDATORY

Build this interface as part of a premium professional trading terminal, not as a
generic SaaS dashboard or AI-generated template.

SOLPRINT uses an all-black visual foundation: neutral near-black surfaces, restrained
grayscale borders, disciplined spacing, intentional hierarchy and minimal decoration.
Do not introduce purple or dark-blue branding, gratuitous gradients, glow effects,
glassmorphism, oversized rounded cards, floating marketing cards, decorative icons,
excessive pills/badges or generic "AI dashboard" aesthetics.

Use color only where it conveys information: P&L, risk, market/chart series,
provider/API health, selected state and explicit operating state. Never rely on color
alone; pair semantic colors with text, signs, icons, line styles or other indicators.

Optimize for eye flow and data density. Prefer aligned workspace panes, tables, grids
and subtle separators over a page full of cards. Keep critical information visually
dominant without giant typography. Use tabular numerals for prices, balances,
percentages, latency and other changing metrics.

Treat shadcn/ui as implementation primitives only. Customize tokens, spacing, radius,
borders, states and composition so the result has a distinct SOLPRINT identity and does
not look like a stock shadcn example.

Desktop is the primary workstation experience, but the interface must degrade coherently
at smaller widths. Critical safety and portfolio information must remain accessible.

Every data component must deliberately support loading, empty, error, stale and
partial-data states. Never invent realistic production metrics. Any fixture/demo data
must be clearly labeled as such.

PAPER versus LIVE trading state must be persistently and unmistakably visible. Live
activation, risk state and emergency-stop controls are safety-critical UI and must never
depend on color or icons alone.

Preserve keyboard usability, visible focus, accessible contrast and semantic markup.

Do not redesign surrounding screens opportunistically. Design-system changes must be
intentional and propagated through shared tokens/components, never as local one-off
styles.

Before considering the frontend task complete:
- render it at the project-standard screenshot viewports
  (1920x1080, 1440x900, 1280x800, 1024x768, narrow monitoring);
- inspect alignment, density, hierarchy, overflow and wrapping;
- test realistic dense data plus loading/empty/error/stale states;
- test keyboard navigation and focus;
- run accessibility checks;
- compare against approved visual baselines;
- explicitly review the screen for generic AI-dashboard traits and remove them.

Functional correctness alone does not satisfy this task. Visual quality is an
acceptance criterion.
```
