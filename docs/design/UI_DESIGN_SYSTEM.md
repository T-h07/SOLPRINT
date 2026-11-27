# UI design system (as implemented)

This documents what exists in code. The binding rules are in
[UI_DESIGN_DIRECTIVE.md](UI_DESIGN_DIRECTIVE.md) and [UI_UX_SPEC.md](UI_UX_SPEC.md);
where this file and the code disagree, `frontend/src/styles/index.css` is authoritative.
A live reference is the internal `/design-system` route (development and showcase builds
only).

## Tokens

Defined once in `@theme` (`frontend/src/styles/index.css`). Tailwind's default palette,
radii, shadows, type scale and breakpoints are **reset**, so only SOLPRINT tokens can be
used. Change styling through tokens and shared components, never one-off values.

### Surfaces and lines (all-black neutral)

| Token | Value | Use |
|---|---|---|
| `canvas` | `#050505` | application background |
| `base` | `#080808` | panels, status bar, navigation |
| `raised` | `#0D0D0D` | controls, popovers |
| `hover` / `pressed` / `selected` | `#141414` / `#1A1A1A` / `#171717` | interaction states |
| `line-subtle` | `#1A1A1A` | decorative separators only |
| `line` | `#252525` | pane borders, table rules |
| `line-strong` | `#333333` | emphasis borders |
| `line-control` | `#666666` | control boundaries (≥ 3:1, WCAG 1.4.11) |

### Foreground (contrast measured on `canvas`…`raised`)

| Token | Value | Contrast |
|---|---|---|
| `fg` | `#F2F2F2` | 16.5–18.2:1 |
| `fg-secondary` | `#A3A3A3` | 7.3–8.1:1 |
| `fg-muted` | `#808080` | 4.66–5.16:1 (the spec's `#737373` failed AA and was raised) |
| `fg-disabled` | `#525252` | disabled controls only, never information |

### Semantic colour — only where it carries meaning, never alone

| Token | Value | Meaning |
|---|---|---|
| `positive` | `#2FBF71` | healthy, available, gain |
| `negative` | `#F2555A` | error, offline, loss |
| `warning` | `#E8A33D` | degraded, stale, unverified |
| `info` | `#6CA6E8` | informational, foundation-only |
| `live` | `#B4232A` (white text 6.5:1) | LIVE trading — reserved |
| `paper` | `#F2F2F2` | PAPER mode — neutral, bordered |
| `focus` | `#70B8FF` | focus ring only, distinct from selection |

Each has a dark `*-surface` variant for tinted backgrounds. Status is always text **plus**
a shape glyph (● filled circle, ○ ring, ▲ triangle, ■ square, ◆ diamond), so meaning
survives without colour (`StatusIndicator`).

### Charts

`chart-1` `#D6D6D6`, `chart-2` `#6CA6E8`, `chart-3` `#3DD3C6`, `chart-4` `#F2A65E`,
`chart-5` `#C9A66B`; grid `#1A1A1A`, axis `#808080`, crosshair `#525252`. Series colours
are not semantic; profit/loss in charts uses `positive`/`negative` plus sign.

### Typography

Inter (variable) for interface text, JetBrains Mono (variable) for identifiers; both
self-hosted via Fontsource (no external font requests).

| Token | Size / line height | Use |
|---|---|---|
| `text-2xs` | 11 / 16 | auxiliary labels; never critical information |
| `text-xs` | 12 / 16 | dense table data, captions |
| `text-sm` | 13 / 20 | default interface text |
| `text-base` | 14 / 20 | emphasised body |
| `text-md` | 15 / 20 | section titles |
| `text-lg` | 20 / 28 | page titles |
| `text-xl` | 24 / 32 | primary metric |

`.num` applies tabular, lining numerals and is required on every financial or operational
number. Number formatting rules: [DATA_CONVENTIONS.md](../architecture/DATA_CONVENTIONS.md).

### Geometry and motion

4 px spacing grid; radius `xs 2`, `sm 3`, `md 4`, `lg 6` (ceiling for containers),
`pill` (state chips only). One overlay shadow. Status bar 44 px; navigation 200 px
(≥ 1200 px) or 56 px icon rail (768–1199 px) or a horizontal strip (< 768 px). Row
heights: compact 32 px, standard 40 px. Motion 80/140 ms with a standard easing; all
durations become 0 under `prefers-reduced-motion`.

### Breakpoints

`sm 480`, `md 768`, `lg 960`, `xl 1200`, `2xl 1440`. Verified viewports: 1920×1080,
1440×900, 1280×800, 1024×768, 390×844.

## Composition

* **Shell:** status bar (identity · operating mode · connectivity · UTC clock) above a
  navigation rail and the workspace. Mode and connectivity are visible at every width.
* **Eye flow on Overview:** safety (operating state) → health → capabilities (modules,
  providers). Sections with no data source are not drawn, rather than filled with
  placeholders.
* **Panels** (`Panel`) are bordered `base` surfaces with a title row and optional
  meta (freshness). Flush panels hold tables edge to edge.
* **Narrow widths** drop secondary table columns and fold their content under the
  primary cell, so the state column is never clipped.

## Components (`frontend/src/components/ui`)

| Component | Variants / states |
|---|---|
| `Button`, `IconButton` | primary, secondary, ghost, destructive; sm 28 / md 32 / lg 40; hover, pressed, selected (`aria-pressed`), disabled, focus-visible |
| `Field`, `Input`, `Select` | label, hint, error (icon + text, `aria-invalid`, `aria-describedby`), disabled |
| `StatusIndicator` | positive, negative, warning, info, neutral — glyph + label + optional detail |
| `Panel`, `Separator`, `DefinitionList` | flush / padded; a `DefinitionList` item with `wrap` breaks long values across lines instead of truncating (used where a value must never be cut off, e.g. cycle dates and denied counts at narrow widths) |
| `Table` (`THead`, `TBody`, `Tr`, `Th`, `Td`) | compact / standard density, numeric right alignment |
| `DataState`, `Skeleton`, `FixtureFrame` | loading, empty, error (with retry), stale, partial; fixtures framed and labelled `FIXTURE` |
| `MetricCell`, `Timestamp`, `DataAge` | loading skeleton; UTC timestamp; age with stale styling |
| `Tooltip` | Radix; keyboard and hover |
| `Drawer` | Radix dialog; focus-trapped; opening focuses the dialog itself (so the close button's tooltip does not capture the first Esc); Esc closes and returns focus to the element that opened it, also for controlled drawers without a trigger |

Shell components (`components/shell`): `AppShell`, `ModeIndicator` (`PAPER MODE`,
`LIVE TRADING`, `MODE UNVERIFIED`, loading), `ConnectionIndicator` (connecting,
connected, degraded, stale, offline), `Wordmark`.

## Gateway workspace (`/gateway`, Phase 01)

`features/gateway/panels.tsx`, laid out by `pages/GatewayPage.tsx` on a three-column grid
at ≥ 1200 px (single column below):

| Pane | Content | Notes |
|---|---|---|
| Helius credit budget (2 cols) | state, credits used of the limit, remaining, admitted priorities; `BudgetMeter`; cycle, linear projection, stream credits/bytes, denied requests per priority, ledger persistence | `BudgetMeter` is `role="meter"` with `aria-valuetext`; threshold ticks (conservation, critical, reserve) are repeated as a text legend, so nothing depends on the bar alone |
| Gateway | enabled, execution capability (`None · read-only access layer`), streams, queue, cache, persistence, uptime | |
| Providers (3 cols) | health with recent-failure ratio, circuit, in flight, queued, requests, errors, 429s, p50/p95/p99, last success | latency figures are histogram bucket bounds and say so |
| Operations (3 cols) | per-operation ok, errors, cache hits, coalesced, retries, percentiles, last error | |
| Usage · last 24 h | stored hourly requests, credits, bytes per provider; process total at ≥ 1440 px | partial state when stored history is unavailable |
| Streams | connection state and generation, last message, bytes and credits, continuity totals, per-subscription continuity | |
| Response cache | per-namespace hits, misses, hit rate, stale serves | |

Labels (`features/gateway/labels.ts`): unmeasured health is shown as **Not measured**,
never as healthy or zero; budget, circuit, stream and continuity states are text plus
glyph. Gateway data polls every 5 s (usage every 60 s) and is marked stale when it
stops updating.

## Data engine workspace (`/data`, Phase 02)

`features/data/panels.tsx` and `features/data/TransactionInspector.tsx`, laid out by
`pages/DataPage.tsx` on a three-column grid at ≥ 1200 px (`xl`; single column below).
Everything on the page is read-only: no control queues, retries or changes work (that is
the `solprint data-engine` CLI).

| Pane | Content | Notes |
|---|---|---|
| Ingestion (2 cols) | queue depth of capacity, stored transactions, swaps and verified swaps, blocking issues; jobs by state, oldest pending, finality and decode-status breakdowns, last decoded, evidence bytes and retention, outbox backlog | when PostgreSQL cannot be read the totals say `database unavailable` and a `PARTIAL` notice explains that nothing is shown as zero |
| Data engine | running / not running, execution capability (`None · read-only decoding`), decoder version, live intake, backfill, USD valuation (off), commitment, process counters since start, last error | |
| Swaps (3 cols) | time (opens the inspector), protocol, wallet with attribution, input and output amounts, out-per-in ratio (≥ 1440 px), decode status with finality | protocol filter (`Select`); below 480 px the amounts stack under the time; retracted swaps say `retracted` and the reason is in the inspector; footnote states that no USD values are shown while price enrichment is off |
| Transactions (2 cols) | signature (opens the inspector), slot, protocols, decode status, finality, swaps, fee | failed-on-chain is text, not colour |
| Data quality | open issues by severity, latest issues with code and detail | |
| Targets (2 cols), Backfills | ingestion targets with coverage and priority; bounded backfills with status and pause reason, pages, queued and duplicate counts, credits | empty states say that targets and backfills are only created by the owner; at ≥ 1200 px the one-third-width Backfills pane folds queued and credits under the page count instead of adding columns |
| Domain events (2 cols) | outbox sequence, event type, quality, aggregate, delivery | |
| Decoder coverage | protocol and verified program id of every registered decoder; decoder version in the header | the registry needs no database, so it stays visible when PostgreSQL is down; a footnote says other programs are stored with their evidence as unsupported |

Transaction inspector (`Drawer`): summary, swaps (venue, status, attribution, sent and
received, ratio, route completeness, USD state, route legs and fees in raw units),
activities in execution order, balance deltas (signed with U+2212, inferred evidence
named), evidence revisions with SHA-256, and quality issues; an unknown signature shows
an empty state, not an error.

Formatting: token amounts are raw `u64` strings formatted with `BigInt`, truncated (never
rounded up) to four places; an unknown scale shows the raw value with `raw`, and an
unknown amount is `—`, never `0` (`formatTokenAmount`, `formatSignedBaseUnits`,
`shortId` in `lib/format/number.ts`). Labels (`features/data/labels.ts`) map every known
decode, finality, attribution, job, severity, backfill and coverage state to text plus
glyph; an unknown future value is shown verbatim in neutral, never coerced into a known
state. Status polls every 5 s and lists every 15 s; lists are marked stale after 45 s.
Shared pane helpers (`LoadFailure`, `Age`, `RowsSkeleton`, `Unreachable`) live in
`features/shared` and are used by both workspaces.

## Data states (mandatory for every data component)

| State | Treatment |
|---|---|
| loading | skeleton sized like the content, `aria-busy`; never a fabricated value |
| empty | neutral glyph + plain statement of why |
| error | `negative` glyph + what failed + safe retry; no stack traces |
| stale | `warning` + age; last known value marked, never presented as current |
| partial | values present shown, missing ones `—` with the reason |
| unverified | for safety state: `MODE UNVERIFIED`, never an assumed default |

## Accessibility standard

WCAG 2.2 AA. Enforced by: axe (Playwright) on every route and the offline state; contrast
values above; visible focus ring (`focus` token, 2 px) on every interactive element; skip
link; one `h1` per page; landmarks (`header`, `nav`, `main`); keyboard-operable
navigation, tooltips and drawers; status conveyed by text and shape, not colour; reduced
motion honoured.

## Visual review requirements

1. Render in a real browser at the five standard viewports.
2. Inspect screenshots (alignment, density, typography, contrast, hierarchy, overflow,
   wrapping, clipping) — do not only generate them.
3. Verify loading, empty, error, stale and offline states, keyboard flow and axe.
4. Visual baselines (`frontend/tests/visual/__screenshots__`) are **candidates until the
   owner approves them**; changed snapshots are never approved blindly.

## Prohibited

Purple/indigo/dark-blue dashboard palettes, gradients, glassmorphism, glow, decorative
illustrations, emoji status, colour-only meaning, unlabelled fixtures, invented metrics.
