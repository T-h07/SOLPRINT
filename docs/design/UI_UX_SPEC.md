# SOLPRINT UI/UX Design Specification

Status: **Adopted** · Applies to: all frontend work · Companion documents:
[UI Design Directive](UI_DESIGN_DIRECTIVE.md) · [Roadmap UI Integration](ROADMAP_UI_INTEGRATION.md)

---

## 1. Design direction

SOLPRINT is a serious trading instrument, not a generic SaaS dashboard with trading
data dropped into cards.

Established financial workstations share a workflow pattern worth borrowing (not their
branding or visual language): the workspace is optimized for **information density,
rapid comparison, persistent context and direct action**. Monitors, analytics, alerts,
order entry and the blotter coexist in one environment, and widgets are modular and
resizable rather than spread across separate pages.

SOLPRINT's identity is quieter and more minimal than those products:

- an almost-black workspace;
- restrained neutral separators;
- typography doing most of the hierarchy work;
- color reserved for meaning.

SOLPRINT already has real-time infrastructure producing continuously changing
operational data (the Helius test received a live, uninterrupted stream of slot
notifications). Provider health, data age and event freshness therefore receive
first-class UI treatment, not decorative status widgets.

### 1.1 Visual hierarchy

Information is ranked in this order. The eye should travel down this list.

| Rank | Layer | Examples |
|---|---|---|
| 1 | Safety and operating state | PAPER/LIVE mode, Guard state, emergency controls, provider degradation |
| 2 | Portfolio state | equity, available capital, exposure, realized/unrealized P&L, drawdown |
| 3 | Actionable intelligence | wallet signals, Cortex opportunities, token security outcomes |
| 4 | Market context | charts, liquidity, prices, volatility, execution conditions |
| 5 | Operational detail | APIs, credits, latency, logs, diagnostics, historical events |

This is **not** implemented as a wall of equally weighted cards.

---

## 2. Visual identity

### 2.1 Foundation tokens

The base theme is **black** — not blue-black, not purple-black.

| Token | Initial value | Role |
|---|---|---|
| `--bg-canvas` | `#050505` | application background |
| `--bg-base` | `#080808` | workspace panes |
| `--bg-raised` | `#0D0D0D` | inspectors, popovers, table headers |
| `--bg-hover` | `#141414` | hover / row highlight |
| `--border-subtle` | `#1A1A1A` | decorative separators |
| `--border-default` | `#252525` | pane borders, table rules |
| `--text-primary` | `#F2F2F2` | primary content and values |
| `--text-secondary` | `#A3A3A3` | labels, secondary content |
| `--text-muted` | ~~`#737373`~~ → **`#808080`** | auxiliary labels (see §2.2) |
| `--text-disabled` | `#525252` | disabled controls only |

These are starting points, not immutable constants. They must be verified for contrast
and on real displays before becoming final design-system tokens.

### 2.2 Contrast audit of the initial tokens

Measured WCAG 2.x contrast ratios (foreground on each background):

| Foreground | canvas `#050505` | base `#080808` | raised `#0D0D0D` | hover `#141414` | Verdict |
|---|---|---|---|---|---|
| primary `#F2F2F2` | 18.21 | 17.89 | 17.36 | 16.46 | pass |
| secondary `#A3A3A3` | 8.08 | 7.94 | 7.70 | 7.30 | pass |
| muted `#737373` | 4.30 | 4.22 | 4.10 | 3.89 | **fails AA (4.5:1) everywhere** |
| muted `#808080` (revised) | 5.16 | 5.07 | 4.92 | 4.66 | pass |
| disabled `#525252` | 2.61 | 2.56 | 2.49 | 2.36 | acceptable — disabled controls are exempt |
| border-default `#252525` | 1.33 | 1.31 | 1.27 | 1.20 | decorative only (see below) |

Consequences:

1. `--text-muted` is revised to `#808080`, the lowest neutral grey that stays above
   4.5:1 on every surface including `--bg-hover`.
2. `--text-disabled` must never carry information the user needs; it is for disabled
   controls only.
3. `--border-subtle` / `--border-default` are suitable only for decorative separators.
   Where a border is the thing that identifies an interactive control (text inputs,
   selects, checkboxes, unselected toggles), WCAG 1.4.11 requires 3:1. Add a
   `--border-control` token at **`#666666`** (3.55 / 3.49 / 3.38 / 3.21 against
   canvas / base / raised / hover), or identify the control by another means.

### 2.3 Semantic colors

Semantic colors are sparse and contextual.

| Meaning | Usage |
|---|---|
| Positive | profit, favorable delta, healthy execution |
| Negative | loss, rejected transaction, critical risk |
| Warning | degraded API, elevated risk, stale data |
| Informational | selected series, neutral system information |
| Live | unmistakable live-trading state |
| Paper | unmistakable simulation state |

**No semantic condition may rely on hue alone** (WCAG 1.4.1). In practice:

- a red loss value still carries `−`;
- a risk condition includes a label or symbol;
- an unhealthy provider says `DEGRADED`;
- charts distinguish important series with line style, markers or labels as well as color.

### 2.4 Prohibited by default

- Purple used as a convenient "tech" accent.
- Blue without an intentional semantic or interaction role.
- Gradients, neon glow, glassmorphism, luminous borders.

---

## 3. Typography and numbers

Use a clean modern sans-serif for interface text, and either the same family with
numeric features or a compatible technical mono/sans for dense numerical regions.

**Tabular numerals are mandatory** for prices, amounts, percentages, latency, slot
numbers, balances and tabular metrics (`font-variant-numeric: tabular-nums`).
Equal-width digits align vertically and keep changing values comparable.

| Role | Size | Notes |
|---|---|---|
| Page title | 20–24 px | no marketing-scale headings |
| Section title | 14–16 px | medium weight |
| Primary metric | 20–28 px | only when genuinely important |
| Default interface text | 13–14 px | |
| Dense table data | 12–13 px | while readability holds |
| Auxiliary label | 11–12 px | never for critical information |

Rules:

- Numbers align right in tables; text aligns left.
- Decimal precision is stable within a column; it does not jump as values update.

---

## 4. Phase 00 design foundation

Frontend initialization alone is not sufficient. Before substantial application
screens are built, Phase 00 establishes:

### 4.1 Design tokens

Centralized tokens for background layers, foreground hierarchy, neutral borders,
semantic states, chart palette, typography, spacing, radii, shadows, sizing, z-index,
motion and focus styles. shadcn/ui's semantic CSS variables are overridden by these
tokens; shadcn is an implementation aid, not the visual identity.

### 4.2 Spacing

4 px base grid. Common values: `4 / 8 / 12 / 16 / 24 / 32`. Density is purposeful:
trading surfaces are denser than configuration or onboarding surfaces.

### 4.3 Radius

Default containers are close to square: **2–6 px**. Not 12–20 px on every panel.
Pill shapes are restricted to states/tags that actually benefit from them.

### 4.4 Primitive states

Every interactive primitive has deliberate states for: default, hover, pressed,
selected, focus-visible, disabled and destructive. Focus-visible is visually distinct
from selected.

### 4.5 Data components

Built before the Terminal is assembled: table primitives, metric cells, timestamps,
currency formatting, percentage deltas, status indicators, panels, tooltips, drawers,
command/search controls, skeletons and error states.

### 4.6 Density variants

Tables support at least **compact** and **standard** density (mature data-table
systems offer row heights from 24 to 64 px).

| Use | Row height |
|---|---|
| High-frequency trading tables | 32–36 px |
| More prominent interaction targets | ~40 px |

### 4.7 Chart system

Chart background, grid, axes, crosshair, series hierarchy, tooltip and
positive/negative conventions are defined centrally as a coherent sub-system, not
themed per chart.

### 4.8 Component showcase

A dedicated internal route or Storybook renders every foundational component with
realistic, clearly labeled trading fixtures in every state. This is SOLPRINT's visual
laboratory and regression reference.

### 4.9 Phase 00 acceptance criterion

Replaced:

> ~~Frontend initialized and application runs.~~

With:

> Frontend foundation is operational and the SOLPRINT design system, token
> architecture, core trading primitives, accessibility baseline and screenshot QA
> harness are established before feature screens are permitted to proliferate.

---

## 5. TERMINAL workspace (Phase 10)

The Terminal is a **workspace shell** supporting monitoring and investigation without
forcing repeated page changes — not a conventional dashboard.

### 5.1 Global shell

| Zone | Size | Contents |
|---|---|---|
| Top status bar | 40–48 px | identity, environment, trading mode, chain/provider status, portfolio equity, global search/command, emergency state |
| Primary navigation | 56–72 px collapsed / 180–220 px expanded | Overview, Wallets, Opportunities, Positions, Replay, API/System, Settings |
| Main workspace | remainder | CSS grid of dense, aligned panes — not isolated floating cards |

Navigation icons are acceptable with tooltips or labels. Decorative icons are not added
to every heading.

Default Overview layout: top = portfolio and safety; middle = equity/P&L and current
positions; bottom = Cortex opportunities and live event feed. Eye flow:
**safety → money → opportunity → system detail**.

### 5.2 Core views

| View | Must provide |
|---|---|
| **Overview** | Within seconds: Is SOLPRINT healthy? Is it safe? What is the portfolio doing? Is there an actionable opportunity? |
| **Wallet Intelligence** | Dense sortable table as primary surface. Per row: score, realized P&L, expectancy, drawdown, win rate, meaningful trade count, recency, copyability, state. Opening a wallet shows a side panel / detail workspace that keeps list context. |
| **Opportunities / CORTEX** | Per candidate: score, expected edge after estimated costs, wallet provenance, signal age, liquidity, Filter decision, Guard constraints, reason codes. No "AI confidence" orb or animated gauge. |
| **Token Security** | Failures and evidence shown structurally: authorities, holder concentration, liquidity conditions, route availability, price impact, rule verdicts. |
| **Positions** | Amount, cost basis, mark, unrealized/realized P&L, age, active exits, Guard state, execution route. |
| **Replay / Sandbox** | Simulated state distinguished from production throughout the page, not only in a header. |
| **System / API** | Helius, Jupiter, DEX Screener and fallback RPC: health, latency distributions, error rate, rate-limit headroom, data freshness, estimated quota consumption. |

### 5.3 PAPER versus LIVE

Safety-critical. Must never depend on a small colored dot.

- PAPER or LIVE is persistently visible in the global shell.
- Entering LIVE requires an explicit, deliberate flow. Never a one-click toggle next to
  normal navigation.
- Entering LIVE has a stronger confirmation than returning to PAPER.

Live-state treatment:

- persistent textual `LIVE TRADING` label;
- semantic danger color;
- clear live wallet identifier;
- Guard status adjacent;
- explicit enabled timestamp;
- persistent emergency stop control;
- no irreversible control based only on an icon.

Emergency stop is visually stable (not constantly flashing), always discoverable and
keyboard-accessible.

---

## 6. Interaction, responsiveness and accessibility

### 6.1 Breakpoints

Desktop-first, not desktop-only.

| Viewport | Behavior |
|---|---|
| ≥ 1440 px | full workstation layout |
| 1200–1439 px | reduced auxiliary panels, core workflow intact |
| 960–1199 px | two-column / collapsible inspector |
| 768–959 px | monitoring-focused, panels stack |
| < 768 px | companion monitoring view; complex workspace simplified |

No core safety information disappears as space shrinks. Complex charts/tables may use
contained horizontal scrolling where their meaning requires it (WCAG 1.4.10 exception
for two-dimensional data); ordinary content reflows.

### 6.2 Keyboard

- `/` or `Ctrl/Cmd+K`: global search / command palette.
- Predictable `Tab` / `Shift+Tab` between components.
- Arrow keys inside grids, menus and tabsets where appropriate.
- `Esc` closes non-destructive dialogs and inspectors.
- Optional, documented shortcuts for high-frequency navigation.
- **No unadvertised single-key shortcut may execute or authorize a trade.**

### 6.3 Pointer targets and focus

- WCAG 2.2 minimum 24×24 CSS px (or its spacing exception). High-risk actions larger.
- Focus is visible and distinct from selected state.

### 6.4 Charts

Charts expose textual values, tooltips and/or legends so important information does not
rely on line color. Positive/negative pairs color with signs, labels or line/marker
differences.

---

## 7. Data states and information integrity

A trading terminal never makes absent, delayed or simulated information look live.

| State | Required treatment |
|---|---|
| Loading | stable skeleton matching content geometry; no fake values |
| Empty | why no records exist and whether that is expected |
| Error | localized error with provider/context; retry when safe |
| Stale | last-updated timestamp plus explicit `STALE` |
| Partial | which providers/data are missing |
| Live | freshness indicator where useful |
| Fixture | unmistakable `FIXTURE` / `DEMO DATA` label |
| Paper | unmistakable simulation label |
| Live trading | unmistakable persistent live label |

Rules:

- No realistic-looking fake balances or P&L in normal screens. Fixtures are allowed in
  development only when explicitly marked and sourced from a fixture layer.
- **Data age is a UI primitive**: `12s ago`, `STALE · 2m 14s`, absolute timestamp in a
  detail tooltip.
- Tables keep column positions while values update. Do not animate every tick or resize
  a column because a value gained a digit.
- Loading never makes panels jump vertically.
- Empty states are compact and operational:
  `No qualified wallets yet. Radar last scanned 38s ago.` — not
  `Nothing here! ✨ Let's get started.`
- Errors distinguish provider failure, server failure and absent data when the backend
  exposes enough information.

---

## 8. Visual quality gate

Functionality is not sufficient for a frontend task to be complete. Every relevant UI
pull request passes three gates.

### 8.1 Automated gate

- Storybook (or equivalent) exercises key states, with automated a11y checks.
- Pixel-based visual regression (Storybook and/or Playwright screenshot comparison).
- Required viewports: **1920×1080, 1440×900, 1280×800, 1024×768**, plus one narrow
  monitoring viewport.
- Baselines cover at least: Overview, Wallets, CORTEX detail, Positions, System/API
  health, live/paper safety states.
- Baselines are generated and compared in a controlled CI environment (screenshot
  rendering depends on environment).
- Automated a11y scanning is a first line of QA and does not replace manual review.

### 8.2 Human visual gate

No frontend phase merges until a human reviews screenshots at actual size:

1. Can the viewer identify trading mode immediately?
2. Is the most important number obvious without being comically oversized?
3. Does the eye naturally progress through the workflow?
4. Are more than two unrelated accent colors competing on one screen?
5. Are panels aligned to common grid lines?
6. Are arbitrary margins or widths creating visual drift?
7. Are cards used where rows, dividers or workspace panes would be better?
8. Is every icon necessary?
9. Is every badge necessary?
10. Is any gradient decorative rather than informational?
11. Does the UI still look coherent with realistic dense data?
12. Does it remain coherent with empty/error/stale data?
13. **Would the screen still look professional with all semantic colors removed?**
    If not, hierarchy depends too much on color.

### 8.3 Anti-slop gate

A UI change automatically fails review if it shows any of these without a strong
functional reason:

- purple/blue gradient as default identity;
- enormous gradient headline;
- glassmorphism panels;
- glowing borders;
- 16 px+ rounded rectangles everywhere;
- metric cards filling the first viewport;
- icons beside every heading;
- decorative sparkle / wand / robot symbols;
- excessive pill badges;
- floating cards with arbitrary shadows;
- empty whitespace added to look "premium";
- generic "Welcome back" hero block;
- marketing-style copy inside the operational workspace;
- shadcn components left in their recognizable stock visual treatment.

---

## 9. Measurable acceptance criteria

Every substantial frontend phase satisfies all of these before merge:

| Criterion | Requirement |
|---|---|
| WCAG text contrast | AA: 4.5:1 normal / 3:1 qualifying large text |
| Non-text contrast | 3:1 for control boundaries and meaningful graphics |
| Color dependence | zero critical states encoded by color alone |
| Keyboard | all functional controls reachable without mouse |
| Focus | visible focus state on every interactive control |
| Pointer targets | WCAG 2.2 24×24 px minimum or valid spacing exception |
| Numeric formatting | tabular numerals in dense financial/operational data |
| Safety state | PAPER/LIVE visible from every trading workspace |
| Emergency control | reachable and recognizable from every live-trading workspace |
| Loading | no fabricated production values |
| Fixtures | all fixtures visibly identified |
| Stale state | time-sensitive regions expose freshness/staleness |
| Responsive | no loss of critical information at defined breakpoints |
| Screenshots | approved baselines for defined viewports |
| Visual regression | unintended screenshot diffs fail CI |
| A11y scan | zero unresolved serious/critical automated findings |
| Human review | explicit visual approval required before merge |
| Template check | no stock shadcn/dashboard appearance accepted |

---

The black theme is the visual identity; **data is what earns color**. SOLPRINT's UI must
communicate safety, capital, opportunities and system state faster than decoration can
distract from them.
