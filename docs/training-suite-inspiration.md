# Training Suite inspiration findings

Observed 2026-10-14 from the authenticated Training Drills page:

- Source: <https://members.blackjackapprenticeship.com/training-drills/>
- Scope: visual and interaction comparison only. No vendor assets, logos, copy, or session data are used by the native app.

## Observed surface

- The menu presents each drill as a card with a short promise, a visual badge, and a distinct start action. Training stats are a separate card with current and lifetime columns.
- Setup is a centered card/modal with grouped settings, toggles/selectors, a speed or duration control, and one prominent start action.
- Active drills use a full felt table. Running Count shows dealer/player cards, a discard tray, tray-angle controls, and a progress/settings panel with timer, penetration, rounds, and accuracy.
- True Count shows a discard tray, a running-count card, a large true-count answer card with stepper controls, a divisor/deck status line, and a countdown/progress panel.
- Basic Strategy shows the dealer hand, player hand, and a visible action grid beside the table. The active surface also keeps timer/progress controls visible.
- Pressure-style play exposes a second-seat/ghost event, payout or mispay feedback, a timed decision state, and a prompt card rather than cycling unrelated questions.
- Stats distinguish current from lifetime performance and expose time, decisions, streaks, and error categories.

## Native implementation mapping

- Reuse the repository's bundled card SVGs and existing felt/table primitives. Added suite-specific card rows are local UI only.
- Running Count now renders automatic local table states, keeps the dealer hole hidden until the simulated dealer turn, and opens an exact count prompt; it does not expose the True Count discard-tray UI.
- True Count renders the depth of its actual discard prefix against a one-deck scale, with three geometric viewing angles and a signed given running count. Exact figures are revealed after grading.
- Strategy drills render cards matching their local situation and legal actions. The answered situation remains until feedback is dismissed; Core Index Plays asks for the explicit comparator as well as action and threshold.
- The active header supplies elapsed/remaining time, graded progress, and a progress bar. Running Count preferences persist the interval and reference delay-based speed scale; other suite settings remain grouped in the native menu.
- Pressure Table owns real payout audits, four-operation arithmetic, an independently played ghost side table with unusual legal actions/occupancy changes, and a five-second decision clock. The side table is explicitly excluded from the learner's shoe and count.
- Training Suite menu and stats now show current/lifetime accuracy, attempts, elapsed time, and streak counters.

## Deliberate differences

- The native app keeps the existing 21 PRO navigation and color system instead of reproducing the website's branding, logos, texture, or external imagery.
- Card visuals come from `assets/cards/`; no remote image URL or copied site asset is introduced.
- The native suite remains offline and persists through the existing profile store; the live site's account/session behavior is not replicated.
