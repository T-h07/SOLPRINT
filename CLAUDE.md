# SOLPRINT — Claude Code instructions

## Frontend / UI work

Any task that creates or materially changes frontend UI must follow
[docs/design/UI_DESIGN_DIRECTIVE.md](docs/design/UI_DESIGN_DIRECTIVE.md) (mandatory) and
[docs/design/UI_UX_SPEC.md](docs/design/UI_UX_SPEC.md). Read both before writing UI code.

Non-negotiables:

- All-black neutral theme; color only where it carries meaning, never color alone.
- Tabular numerals for all financial and operational numbers.
- PAPER / LIVE state persistently visible; live activation and emergency stop are
  safety-critical UI.
- Every data component handles loading, empty, error, stale and partial states. Never
  fabricate production values; fixtures are labeled `FIXTURE` / `DEMO DATA`.
- Change styling through shared tokens/components, not local one-off styles. Do not
  redesign surrounding screens opportunistically.
- A UI task is not complete until it has been rendered at the standard viewports and
  checked against the quality gates in the spec (§8–9).
