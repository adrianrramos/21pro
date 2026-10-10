# 21 Pro — Project Vocabulary

Shared vocabulary for the application's areas, blackjack concepts, learning tools, and future feature work. Terms marked **Proposed** are working names rather than approved app-tab renames or suite boundaries; terms marked **Planned** describe capabilities that are not yet implemented.

Implementation components and libraries are defined separately in the [technical companion](README.md#component-vocabulary). Current behavior is documented in the [README](README.md#the-training-loop); future capabilities are described in the [roadmap](roadmap.html).

## Language

### Application structure

**21 Pro**:
An offline desktop application for learning blackjack through simulated play, decision feedback, performance analysis, and personalized practice. It does not provide real-money gambling.

**Suite — Proposed**:
A user-facing collection of related tools serving a common learning goal.

**Tool — Proposed**:
A focused capability within the application, such as a decision heatmap, targeted drill, or card-count trainer. A tool does not necessarily need its own app tab.

**Application tab (app tab)**:
A clickable navigation item in the application's sidebar that switches the main view to a specific application area. “The table,” “Your insights,” “Practice,” “Card counting,” and “Rules” are each app tabs; the sidebar is the container that holds them.

**Study mode**:
The learning context in which a blackjack decision is recorded. The current modes are table play and focused practice; their results can be analyzed separately or together.

**Live — Proposed name for “The table”**:
The naturally dealt blackjack experience, where the learner plays complete rounds and receives immediate strategy feedback. “Live” would mean interactive simulated play, not an online casino, multiplayer connection, or human dealer.

**Practice**:
The application area for deliberately revisiting selected situations and scheduled reviews. Unlike Live/table play, its starting situations are chosen for learning rather than encountered through unrestricted dealing.

**Insights**:
The application area for understanding recorded performance through accuracy, trends, hand-family breakdowns, decision heatmaps, and suggested practice priorities. Its current app-tab label is “Your insights.”

**Rules**:
The reference area explaining the supported blackjack rules, strategy sources, statistical meanings, and local data storage. It is not currently a ruleset editor.

### Live and blackjack

**Ruleset**:
The collection of rules governing legal actions, dealer behavior, and settlement. The current ruleset is a historical Yaamava six-deck snapshot, not a claim about the casino's present-day tables.

**Shoe**:
The finite supply of playing cards from which rounds are dealt. The current game uses six decks and reshuffles between rounds when the remaining cards reach the cut threshold.

**Round**:
One original deal through its final settlement, including any hands created by splitting. A round can contain several hands and many decisions.

_Avoid_: Using “round” and “hand” interchangeably when counting progress.

**Hand**:
The cards belonging to one player hand or the dealer. Splitting creates additional player hands within the same round.

**Hand family**:
A strategy and analysis grouping: hard totals, soft totals, or pairs. A soft total counts an ace as 11; a hard total does not.

_Avoid_: Calling actions such as doubling and splitting “hand families.”

**Action**:
A move available to the learner, such as hit, stand, double, split, surrender, or an insurance choice. Which actions are legal depends on the current situation.

**Decision attempt**:
One chosen action recorded for evaluation against basic strategy. Follow-up choices after hitting or splitting are separate attempts; a completed round does not automatically equal one attempt.

**Situation**:
The decision context formed by the player's hand family and value, the dealer's visible card, and the relevant legal-action restrictions. “16 against 10” alone is not a complete situation.

**Legal-action context**:
The restrictions that determine which moves are available, including whether the hand has already been hit or split and whether further splitting is allowed. Similar-looking hands can require different recommendations because their legal actions differ.

**Basic strategy**:
The ruleset-appropriate recommendation for a situation without using the running count, remaining-shoe composition, or the dealer's hidden card. It is the standard against which current decision attempts are scored.

**Decision feedback**:
The assessment of the learner's chosen action, including the recommended action and its explanation. An incorrect but legal choice is still played; feedback does not replace it with the correct move.

**Round outcome**:
The simulated result after settlement. It is separate from decision quality: a winning round can contain strategy mistakes.

### Learning and Insights

**Baseline**:
The initial sample of 250 completed table rounds required to unlock the assessment and personalized practice. It accumulates across app sessions; practice rounds do not advance it.

**Assessment**:
The analysis made available once the baseline is complete. It summarizes observed strengths and weaknesses rather than administering a separate exam or certifying mastery.

**Decision accuracy**:
The proportion of recorded attempts that match the recommended action. It measures strategy choices, not wins or profitability.

_Avoid_: “Win rate” or “mastery score” as synonyms.

**Decision heatmap**:
A grid showing observed mistake rates by hand family/value and dealer upcard. A cell can contain several distinct legal-action contexts; an unobserved cell means “not yet measured,” not “mastered.”

**Accuracy trend**:
The sequence of accuracy measurements across consecutive groups of decisions. The current application uses non-overlapping blocks of 25 decisions and identifies the final partial block.

**Weak situation**:
An observed situation prioritized for further practice using mistake history and sample size. A high priority is a learning signal, not proof of a statistically established weakness.

**Targeted drill**:
Practice beginning from a selected situation, such as one inspected through the heatmap. The hand continues after the targeted decision, so later choices are also recorded.

**Practice plan**:
An ordered selection of situations for a practice session. The current plan includes up to 12 distinct situations, prioritizing due reviews before filling remaining positions with weak observed situations.

**Review card**:
A scheduled learning item representing an exact situation and its review history. It is created after an incorrect answer and is distinct from a playing card.

_Avoid_: Unqualified “card” when discussing the review schedule.

**Due review**:
A review card whose scheduled review time has arrived or passed. Answering correctly before that time still records the attempt but does not push the scheduled review farther away.

**Spaced repetition**:
The learning approach that schedules situations for repeated practice over time, with errors returning sooner and successful due reviews receiving longer intervals.

**Player profile**:
The locally saved record of decision attempts, completed table rounds, review schedules, and correct counting trials. It is not an online account and does not resume an unfinished hand or counting trial after the app closes.

### Card counting

**Card-count trainer**:
The speed-and-accuracy tool in the “Card counting” app tab, presenting 52 cards from a freshly shuffled six-deck shoe and checking the learner's final Hi-Lo running count. Only correct trials are saved, with the fastest history first.

**Running count**:
The cumulative tally of exposed cards according to a card-counting system. The current card-count trainer uses Hi-Lo; the running count does not alter the app's basic-strategy recommendations.

**Counting trial**:
One timed 52-card counting exercise followed by a final running-count answer. Trials are separate from blackjack rounds and do not advance the baseline or contribute to decision-accuracy statistics.

### Planned tools and capabilities

**Hand-value display — Planned setting**:
The visibility of the app's calculated hand total during practice. A planned setting would let learners hide it so they must total the cards themselves.

**Practice focus — Proposed name; planned capability**:
A learner-selected restriction on the kinds of exercises generated, such as soft hands, splitting situations, or doubling opportunities. This broader filtering capability is distinct from the existing drill for one exact situation.

**Running-count check — Planned**:
A periodic in-play prompt asking the learner to report the cumulative count of exposed cards. This planned check would evaluate the answer and record counting accuracy during play, rather than only at the end of a separate counting trial.

**Bankroll simulation — Planned**:
A planning tool using inputs such as intended playing time, starting bankroll, and bet size to estimate expected value and variance. Its estimates would describe possible financial behavior, not predict or guarantee a particular casino result.
