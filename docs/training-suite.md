# Training Suite

## Implementation

- Keep blackjack mechanics in `game`/`model`, count arithmetic in `counting`, strategy and local indexes in `strategy`/`training_suite`, persisted summaries in `training::Profile`, and rendering/intent routing in `app`.
- Add one top-level Training Suite page with a native menu, setup cards, answer feedback, and reports. Existing Table, Practice, Card counting, Insights, and Rules routes remain unchanged.
- Use the current H17/DAS/late-surrender ruleset only. The suite owns its attempts and settings; it never adds drills to the existing heatmap or 250-round baseline.
- Implement pure count, catalog, index, grading, assessment, pressure, and stats functions first; the renderer only displays state and sends intents.
- Read profile schemas 1 and 2 with defaulted suite fields and save schema 3. Existing rounds, decisions, reviews, and counting history remain untouched; active drills are not resumed after restart.

## Core Index Plays provenance

The local catalog contains the 18 Illustrious 18 matchups and four Fab 4 matchups, with **six-deck Hi-Lo, H17, DAS, late surrender** values. This is the suite's only supported index baseline; it does not advertise S17, ENHC, other deck counts, expanded indexes, or risk-averse indexes. Ordinary Table recommendations still call the unchanged H17 basic-strategy chart without indexes. Index data is normalized locally, shared by the drill and suite table through `strategy::core_index_plays` / `graded_action`, and never fetched at runtime.

Sources independently consulted on 2026-10-25:

1. [Wizard of Odds, *Introduction to the High-Low Card Counting Strategy*](https://wizardofodds.com/games/blackjack/card-counting/high-low/): reproduces Schlesinger's I18/Fab 4 with permission, including the two ten-splits, four actual surrender matchups, inclusive high-side comparisons, and insurance at ≥ +3. Its published table is the familiar S17 set, not an exact H17 source.
2. [QFIT, *Playing Strategy (Simple)*](https://www.qfit.com/CVBlackjackHelp/playingstrategysimple.htm), [*Modern Blackjack*, p. 383](https://www.qfit.com/book/ModernBlackjackPage383.htm), and [p. 397](https://www.qfit.com/book/ModernBlackjackPage397.htm): establish I18 versus Sweet 16 (I18 minus ten-splits), the Fab 4 category, rule-dependent index tables, and six-deck H17/S17 Hi-Lo truncation simulations. These pages do **not** independently enumerate every numeric row; the former documentation's purported QFIT per-row corroboration was incorrect.
3. [Blackjack Trainer, *Blackjack H17 Deviation Chart*](https://www.blackjacktrainer.fyi/charts/deviations): explicitly lists six-deck H17/DAS/late-surrender I18 and Fab 4 values alongside S17, with both sides of each comparison and surrender precedence.
4. [Blackjack Strategy, *Blackjack Deviation Charts*](https://blackjackstrategy.io/strategy/deviations): independently describes its composition-dependent six-deck calculation and publishes H17 charts. These corroborate the H17-specific crossovers and their strict low-side equivalents. Its published I18/Fab 4 summary corroborates the conventional numbers; its separate engine-calculated chart intentionally differs on some other rows.

The table below records the **encoded condition/action**, not a copied chart. Where the chart already recommends the high-side action, the row stores the inverse strict-low-side deviation. This makes ordinary basic strategy the real fallback rather than incorrectly standing/doubling/surrendering below the crossover.

| Category | Player / dealer | Encoded true-count condition | Action | Additional legality |
| --- | --- | --- | --- | --- |
| I18 | Insurance / A | ≥ +3 | Insure | Insurance phase only |
| I18 | Hard 16 / 10 | ≥ 0 | Stand | Does not override legal chart surrender |
| I18 | Hard 15 / 10 | ≥ +4 | Stand | Does not override legal chart surrender |
| I18 | Pair 10,10 / 5 | ≥ +5 | Split | Split available |
| I18 | Pair 10,10 / 6 | ≥ +4 | Split | Split available |
| I18 | Hard 10 / 10 | ≥ +4 | Double | Double available |
| I18 | Hard 12 / 3 | ≥ +2 | Stand | — |
| I18 | Hard 12 / 2 | ≥ +3 | Stand | — |
| I18 | Hard 11 / A | < -1 | Hit | At/above -1, chart doubles if legal, otherwise hits |
| I18 | Hard 9 / 2 | ≥ +1 | Double | Double available |
| I18 | Hard 10 / A | ≥ +3 | Double | Double available |
| I18 | Hard 9 / 7 | ≥ +3 | Double | Double available |
| I18 | Hard 16 / 9 | ≥ +5 | Stand | Does not override legal chart surrender |
| I18 | Hard 13 / 2 | < -1 | Hit | — |
| I18 | Hard 12 / 4 | < 0 | Hit | — |
| I18 | Hard 12 / 5 | < -2 | Hit | — |
| I18 | Hard 12 / 6 | < -3 | Hit | — |
| I18 | Hard 13 / 3 | < -2 | Hit | — |
| Fab 4 | Hard 14 / 10 | ≥ +3 | Surrender | Late surrender available |
| Fab 4 | Hard 15 / 9 | ≥ +2 | Surrender | Late surrender available |
| Fab 4 | Hard 15 / 10 | < 0 | Hit | Late surrender available; chart surrenders at/above 0 |
| Fab 4 | Hard 15 / A | < -1 | Hit | Late surrender available; chart surrenders at/above -1 |

**Source differences and limits:** Wizard's S17 values are 11/A +1, 10/A +4, 12/6 -1, and surrender 15/A +1; the shipped H17 crossovers are respectively -1, +3, -3, and -1, corroborated by the two H17 sources. Blackjack Strategy's independent engine puts 16/10 stand at +1 and 16/9 at +6; its explanation distinguishes a two-card, mid-shoe calculation from conventional published multi-card I18 indexes. The project selects the conventional 0/+5 indexes corroborated by Wizard and Blackjack Trainer, not a mixture of the engine's alternatives. Insurance remains the conventional +3, not the engine's fractional break-even estimate. Strict `<` is the complement of the selected inclusive `≥`, including at zero and negative thresholds.

Fab 4 is **14/10, 15/10, 15/9, 15/A**, not 14/9 or 14/A; 15/9 is surrender, not stand, and 16/8 is outside I18/Fab 4. Ten-splits are included, so the catalog is not misnamed Sweet 16. No shipped row is DAS-only: splitting tens itself does not require a double after split; the supported game nevertheless has DAS. Legal chart surrender takes precedence over ordinary hit/stand indexes, legal chart splits over hard-total hit/stand/double indexes, and matching surrender indexes over split/double. Hard-total rows also cover hard pairs when a higher-priority chart split does not apply. Split aces retain their one-card/resplit restriction. Out-of-catalog count changes deliberately fall back to the local chart; this is not a complete count-dependent strategy. The suite's stepped divisor and truncation-toward-zero are local training conventions, not a claim that all sources use identical count estimation or rounding.

## Scope notes

- Running Count persists the reference delay scale (800 ms displays as dealing speed 6) alongside the checkpoint interval; cards slide into each state and clear after a three-second recall pause once a round settles.
- Running Count automatically plays each round with the local basic strategy and dealer H17 flow. The hole is excluded while face down, then revealed before visible dealer hits. A one-shoe final count is required before the engine's automatic shuffle; an incorrect final answer remains visible in the report.
- Shared card rows retain centered, spaced slots throughout the staggered slide, keeping player cards and the dealer upcard/hole card visually distinct.
- True Count shows the signed given running count before submission and preserves it when changing tray angle. Enter submits suite numeric answers even while their text editor is focused; other focused-editor shortcuts remain disabled.
- True Count renders the actual discard depth at three angles, with a one-deck reference scale. Exact card/divisor figures appear after grading; no control supplies the alternate answer. Whole/half-deck divisors truncate toward zero, including negative counts; whole-deck boundary rules take precedence at the quarter-deck endpoints. A nonempty perfect session passes when its timer expires.
- Basic Strategy generates only actionable hands whose rendered cards match their total, family, and legal actions. The answered hand stays on screen until feedback is dismissed; a second action cannot grade the same answer again.
- Core Index Plays requires the action, explicit comparator, and threshold. Strict low-side plays are graded as the named rule, not by evaluating the basic-strategy fallback at the equality boundary.
- Table Practice accounts for both automatic natural settlements and played settlements. Every numbered interval and end-of-shoe mode requires a final count before the existing engine's 68-card shuffle threshold; the original Table rules are unchanged. Suite table decisions accept the documented quarter-deck alternative.
- Assessment is distinct from the existing 250-round baseline. It locks five-round checks, requires exact final counts across three complete shoes, and passes only when every answer is correct. Early exits and duplicate submissions cannot become passes.
- Pressure Table has real payout audits (correct payments as well as occasional mispays), four-operation integer math, and a five-second human-decision clock. Unanswered events cannot be overwritten by another deal; ghost/count prompts do not consume decision time. The **ghost side table uses its own shoe**, plays complete local rounds with occasional unusual legal choices, and does not contribute to the learner's running count.
- Current-stat resets preserve lifetime history. Suite settings, counters, elapsed time, and completed/incomplete summaries survive database reopen without changing the old baseline or Insights attempts.

## Verification

Required checks: `cargo fmt --all --check`, `cargo check --locked`, `cargo test --locked --all-features`, and `cargo clippy --locked --all-targets --all-features -- -D warnings`.

Verification uses disposable profiles, never personal progress. The actual native app was exercised through loopback egui inspection on Xvfb/Mesa: automatic Running Count through its final prompt, a timed True Count pass, all 22 index answers and report, retained Basic Strategy feedback, Table Practice count submission/continuation, Assessment early exit, pressure timing/audit/math events, a narrow viewport, and settings/current/lifetime counters after restart. A separate real-domain smoke completed the full Basic Strategy catalog, a three-shoe perfect Assessment, and repeated pressure rounds. This is Linux native-render proof, not Windows/WSLg or macOS verification.
