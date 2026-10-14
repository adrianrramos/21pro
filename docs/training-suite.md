# Training Suite

## Implementation plan

- Keep blackjack mechanics in `game`/`model`, count arithmetic in `counting`, strategy and local indexes in `strategy`/`training_suite`, persisted summaries in `training::Profile`, and rendering/intent routing in `app`.
- Add one top-level Training Suite page with a native menu, setup cards, answer feedback, and reports. Existing Table, Practice, Card counting, Insights, and Rules routes remain unchanged.
- Use the current H17/DAS/late-surrender ruleset only. The suite owns its attempts and settings; it never adds drills to the existing heatmap or 250-round baseline.
- Implement pure count, catalog, index, grading, assessment, pressure, and stats functions first; the renderer only displays state and sends intents.
- Migrate old profiles with serde defaults and a schema bump. Existing rounds, decisions, reviews, and counting history remain untouched.

## Core Index Plays provenance

The initial local index set is the project-selected Hi-Lo Illustrious 18 plus Fab 4 for the current H17/DAS/late-surrender ruleset. Each row is normalized as hand class, value, dealer upcard, comparator, threshold, action, and rule flags. The table is not a copied chart and is not a runtime dependency.

Every shipped threshold and comparator was independently checked against both sources below:

1. Wizard of Odds, *Blackjack Card Counting* and index-play discussion: <https://wizardofodds.com/games/blackjack/card-counting/>
2. QFIT / Norm Wattenberger, *The Illustrious 18 and Fab 4*: <https://www.qfit.com/book/modern-blackjack-illustrious-18.htm>

The shipped set is exactly insurance +3; I18 hard plays 16v10 +0, 15v10 +4, 10v10 +4, 12v3 +2, 12v2 +3, 11vA +1, 9v2 +1, 10vA +4, 9v7 +3, 16v9 +5, 13v2 -1, 12v4 +0, 12v5 -2, 12v6 -1, 13v3 -2, 15v9 +2, and 16v8 +4; and Fab 4 surrender plays 14v9 +5, 14vA +3, 14v10 +3, and 15v10 +0. No pair-10 rows are included. Expanded deviations, ENHC, S17, and arbitrary deck variants are intentionally not selectable.

## Scope notes

- Running Count excludes a face-down hole until reveal and uses exact integer grading.
- True Count uses stepped whole/half-deck divisors, truncation toward zero, and the documented quarter-deck alternate.
- Assessment is a local three-shoe suite session and is distinct from the existing 250-round baseline assessment.
- Pressure Table derives ghost-seat occupancy/actions, payout mismatch values, and a five-second decision clock from the local table state; prompts are not synthetic rotations of unrelated booleans.
