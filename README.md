# 21 Pro

An offline, native Rust desktop app for learning blackjack basic strategy. Play real rounds, see mistakes immediately, and turn your own weak decisions into spaced-repetition practice.

All application code—including the UI, blackjack engine, analytics, scheduler, and persistence—is Rust. There is no JavaScript frontend, WebView, server, account, or telemetry. Rust dependencies still use the operating system's native graphics, windowing, and accessibility APIs.

Playing-card faces and backs use bundled SVG artwork exported from the full [CardMeister](https://cardmeister.github.io/index.html?full) set. The assets are rendered natively with egui's SVG loader and cached by egui; the app does not use a webview or fetch artwork at runtime. See [`assets/cards/README.md`](assets/cards/README.md) for the upstream revision and Unlicense provenance.

## Run on your Mac

Requirements: macOS 12 or newer, Xcode Command Line Tools, and Rust **1.95 or newer**. Both Apple Silicon and Intel Macs use the same source.

1. Install Xcode Command Line Tools if needed: `xcode-select --install`.
2. Install Rust using [rustup](https://rustup.rs/), or update an existing installation with `rustup update stable`.
3. Clone and launch from Terminal:

```sh
git clone https://github.com/adrianrramos/21pro.git
cd 21pro
cargo run --locked --release
```

The first build downloads and compiles dependencies. The app works offline afterward. Do not run Cargo or the app with `sudo`.

### Build a Finder-launchable `.app`

Run these commands **on your Mac**:

```sh
cargo install cargo-bundle --version 0.12.0 --locked
cargo bundle --release
open "target/release/bundle/osx/21 Pro.app"
```

You can copy `21 Pro.app` into your Applications folder. This is a locally built, unsigned app; publishing signed/notarized downloads is separate from building and running your own copy. The bundle configuration is in `Cargo.toml`.

### Linux development

The same native app also runs on Linux with an X11 display, a C linker, and OpenGL/EGL runtime libraries. On Debian/Ubuntu, install `build-essential pkg-config libxkbcommon-x11-0 libgl1 libegl1`, then use the same Cargo command. A headless server needs a display such as Xvfb. No Linux-only libraries are required on macOS.

#### WSLg development over SSH

WSLg needs a working Windows desktop connection as well as an X11 socket. On this host, starting WSL from SSH without a signed-in Windows desktop left WSLg's `msrdc.exe` in noninteractive Session 0. X11 connections timed out and Weston repeatedly exited with signal 11. Restarting WSL from the signed-in desktop moved `msrdc.exe` to Session 1 and restored native rendering. This is an observed recovery, not a diagnosis of every Weston crash.

If this happens:

1. Sign into the Windows desktop, locally or through Windows remote desktop.
2. Save work in **all WSL sessions**. From Windows Terminal on that desktop—not over SSH—run:

   ```powershell
   wsl --shutdown
   wsl -d Ubuntu
   ```

   Shutdown stops all WSL processes and disconnects agents. Leave the new Ubuntu terminal open, then reconnect Orca/SSH.
3. Verify the socket mapping without modifying it:

   ```sh
   stat -Lc '%d:%i %n' /tmp/.X11-unix/X0 /mnt/wslg/.X11-unix/X0
   findmnt -T /tmp/.X11-unix
   ```

   Matching device/inode pairs identify the same socket. A read-only mount is valid; the directory does **not** have to be a symlink. Do not delete or replace a working mount. Socket presence alone does not prove the display server responds.
4. Install the Linux dependencies listed above, including `libxkbcommon-x11-0`. In the isolated development worktree, launch with a disposable profile:

   ```sh
   sandbox=$(mktemp -d)
   DISPLAY=:0 TWENTY_ONE_PRO_DATA_DIR="$sandbox" cargo run --locked
   # After closing the app:
   rm -r -- "$sandbox"
   ```

   SSH sessions may lack `DISPLAY`; this command sets it only for the WSLg launch. Do not overwrite a deliberately configured SSH-forwarded or headless display.
5. Confirm the actual window renders, then press Enter to deal and check that the cards and action buttons update. Successful compilation or a socket check is not visual verification.

If connections still fail, inspect `/mnt/wslg/stderr.log` and `/mnt/wslg/weston.log`. From Windows PowerShell, `Get-Process explorer,msrdc | Select-Object ProcessName,Id,SessionId` helps distinguish the desktop and service sessions. Do not repeatedly restart WSL or rewrite socket paths without checking the failure.

PR 1 verification exercised the native Glow/X11 app at 1180×860 on WSLg: captured the initial table, sent window-directed Enter input, and captured the dealt hand with action buttons. The profile was isolated from personal progress. This does not verify the later inspection/MCP integration or a Windows/macOS native build.

References: [Microsoft WSL GUI support](https://learn.microsoft.com/en-us/windows/wsl/tutorials/gui-apps), [WSLg architecture](https://github.com/microsoft/wslg#architecture), and [X11 connection diagnostics](https://github.com/microsoft/wslg/wiki/Diagnosing-%22cannot-open-display%22-type-issues-with-WSLg).

## The training loop

1. **The table:** play a finite six-deck shoe. Hit, stand, double, split, surrender, and make insurance decisions. Incorrect choices are explained immediately, but your chosen move is still played. A lucky win does not turn a mistake into a correct answer.
2. **Baseline:** complete **250 original table rounds**, accumulated across sessions. Split children are part of their original round; practice never advances this threshold. Naturals count as completed rounds, but produce no artificial decision score.
3. **Your insights:** the assessment opens automatically at the threshold. It includes accuracy trends, mistakes by hand family, and separate hard-total, soft-total, and pair heatmaps. Table play and focused practice can be viewed separately or together.
4. **Focused practice:** generate a 12-hand plan from due reviews and weak observed situations, or drill a specific heatmap situation. Practice uses the same engine, and hands continue after the targeted decision. If fewer than 12 distinct situations are known, the plan uses the available ones.
5. **Card counting:** open the Card counting tab for a 52-card Hi-Lo trial from a freshly shuffled six-deck shoe. The final running count is checked after timing stops; only correct trials are saved, with the fastest history first.
6. **Free Play:** open Play without completing the training baseline. Choose a simulated bankroll, build a wager with red $5, green $25, black $100, or yellow $1,000 chips, and play the same fixed six-deck rules. The adjacent session graph records one settled point per original round.
7. **Repeat over time:** mistakes enter a local spaced-repetition schedule. Due correct answers earn longer intervals; errors return sooner. History, due dates, counting trials, and the current Free Play shoe/session survive closing the app.

### Controls

| Key | Action |
| --- | --- |
| Enter | Deal / next hand / finish a practice session |
| Space | Next card or finish the active card-counting trial |
| H | Hit |
| S | Stand |
| D | Double |
| P | Split |
| R | Surrender |
| I / N | Take / decline insurance |

Buttons expose the same actions. Illegal moves are disabled. Feedback stays visible in a fixed panel even when the table scrolls. AccessKit support is enabled; drawn card groups have accessible descriptions that do not reveal the dealer's hidden card.

## Exact rules and provenance

This release trains **Yaamava's six-deck game as reported in the February 2025 issue of Current Blackjack News**. The Yaamava entry itself is dated **December 2024**. It is a historical rules snapshot, not a claim about the casino's current tables.

| Rule | Implemented behavior |
| --- | --- |
| Shoe | Six decks; shuffle before a round at 68 or fewer cards remaining, approximating the reported 1.3-deck cut |
| Dealer | Hits soft 17 |
| Natural blackjack | 3:2; blackjack after a split is an ordinary 21 |
| Double | Any first two cards, including after splitting non-aces; exactly one additional card |
| Splitting | Up to four total hands, including resplitting aces |
| Split aces | One card each; no hit/double, but a new pair can be resplit within the limit |
| Surrender | Late, original two-card hand only; not after a hit or split |
| Insurance | Half the initial wager, paying 2:1; basic strategy always declines |
| Strategy | Total-dependent 4–8 deck H17, double after split, late surrender; no counting or composition-dependent deviations |

### Hole-card timing matters

The survey's defaults say the dealer does not look at the hole card and that only the original main bet is lost to dealer blackjack. The simulator therefore reveals a dealer natural at settlement and refunds **all additional split/double stakes**, including extra stakes on busted hands. Insurance settles separately. A late-surrender request is conditional: it costs half the original bet only when the dealer does not have blackjack; otherwise the original main bet is lost. Legality and recommendations never inspect the hidden rank.

Practice preserves the exact legal-action context. A three-card hard 11 is not a two-card doubling opportunity. A post-split 16 cannot be surrendered. A split-limit exercise reserves already-used split capacity without inventing extra active hands. Forced practice cards are removed from a real six-deck shoe; subsequent cards are dealt normally.

Sources:

- [Supplied CBJN report](https://assets.bj21.com/newsletters/pdf_files/000/000/113/original/CBJN2502.pdf?1648784494): Yaamava page 13; default rules page 2; abbreviations page 55.
- [Wizard of Odds, 4–8 deck strategy](https://wizardofodds.com/games/blackjack/strategy/4-decks/): the **H17 image** is the strategy reference. The prose on that page describes **S17**, so it must not be substituted for the H17 chart.

The newsletter and chart artwork are not distributed with this repository. This project is not affiliated with Yaamava, BJ21, Wizard of Odds, or Anki. Basic strategy does not eliminate the house edge. There is no real-money play.

## How learning is measured

- **One attempt = one chosen action.** Follow-up decisions after hits and splits are recorded, not just starting hands. No free retries inflate accuracy.
- **Unseen is not mastered.** Heatmap cells with no observations show a dash. Hover a cell for its mistake/sample counts; click it to see the distinct legal-action contexts inside it.
- **Weakness ranking:** `(mistakes + 1) / (attempts + 5)`, a Beta(1,4) prior that tempers tiny samples. Displayed percentages remain the actual observed rates. All-correct histories prioritize less-observed situations.
- **Progress:** consecutive, non-overlapping blocks of 25 decisions. A final partial block is explicitly marked. These are decision-order trends, not a claim of statistical significance or a calibrated mastery score.
- **Review identity:** hand family/value, dealer upcard, and legal double/split/surrender/split-ace context. Different correct answers never share a review card simply because the visible total matches.

### Spaced repetition

The scheduler is **SM-2-style, not FSRS and not an Anki integration**:

- The first incorrect answer creates a review card.
- An error schedules relearning in 10 minutes, resets successful repetitions, and lowers ease by 0.32, to a minimum of 1.3.
- A correct answer when due schedules 1 day, then 6 days, then the previous interval multiplied by ease and rounded to whole days.
- A correct early practice answer is still recorded but **does not postpone the due date or inflate repetitions**.
- Due situations come first in a practice plan; weak observed situations fill the remaining positions.

The clock is passed into learning methods as a Unix timestamp, so scheduling tests do not sleep or depend on the real time of day.

## Local data and recovery

On macOS, the database is:

```text
~/Library/Application Support/dev.TwentyOnePro.21Pro/profile.redb
```

The Rules screen shows the exact path on every platform. Close the app before copying that file as a backup. A single versioned JSON profile and the current versioned Free Play session are committed transactionally inside a `redb` database. A locked, corrupt, wrong-ruleset, or unsupported-version profile or Free Play snapshot produces an explicit error; the app does not silently replace it with empty progress.

Decisions, completed rounds, review schedules, and correct card-counting trials are saved locally. Free Play saves its starting bankroll, exact cents accounting, graph history, undealt shoe order, dealer/player cards, active phase, split/insurance state, and unfinished round after every accepted change, so closing and reopening resumes the same session without redealing or duplicate settlement. If a write fails, play pauses, a retry is offered, and closing warns about unsaved changes.

For an isolated development profile:

```sh
TWENTY_ONE_PRO_DATA_DIR=/tmp/21pro-sandbox cargo run --locked
```

No test profile, generated baseline, or personal database ships with the app.

## Rust walkthrough

This is one Cargo package with a library and a desktop binary. The library exposes the mechanics and learning logic without depending on UI state. The renderer calls those modules; it does not implement blackjack rules itself.

| Read in this order | Rust concepts to study | What to trace |
| --- | --- | --- |
| `src/model.rs` | Enums, `match`, `Copy`, derives, borrowed slices | How multiple aces change from 11 to 1; why a `Situation` includes legal actions |
| `src/counting.rs` | Seeded shuffles, bounded state transitions, serde records | How a six-deck trial reveals exactly 52 cards and applies Hi-Lo values |
| `src/strategy.rs` | Pure functions, match guards, static string references | Surrender/split/double precedence and fallback when an action is unavailable |
| `src/game.rs` | Ownership, `&mut self`, `Result`, state transitions, seeded RNG | Follow `deal`, `situation`, and `act`; find where a chosen double draws exactly one card |
| `src/training.rs` | Iterators, `BTreeMap`, `BTreeSet`, deterministic state updates | Follow an incorrect attempt into history, analytics, and the review queue |
| `src/storage.rs` | Serde, typed errors, RAII, transaction lifetimes, `?` | Why the table guard goes out of scope before the write transaction commits |
| `src/app.rs` | Borrowing across modules, application state, command dispatch | Capture the pre-move situation, apply the action, record it once, then save |
| `src/app/views.rs`, `src/app/widgets.rs` | Closures, immediate-mode UI, custom painting | How rendering reads state and emits a command without replaying game actions every frame |
| `src/main.rs` | Native application entry point, configuration | How the window and `TrainerApp` are created |

`Analytics::trend` reuses `CellStats` for nonempty blocks of up to 25 decisions; the renderer gets block numbers from their positions. Analytics and the UI use `Option<StudyMode>`: `Some(mode)` selects one mode and `None` selects both. The controller owns table/practice identity, while card-counting history is stored separately and never contributes to blackjack learning analytics. The saved profile remains backward-compatible because older snapshots default the new history to empty.

### A concrete decision to follow

For an original two-card hard 16 against a dealer 10, the available late surrender is recommended. If you choose **Stand**:

1. `Game::situation` describes the hand **before** it changes.
2. `strategy::recommendation` returns surrender and an explanation.
3. `Game::act(Action::Stand)` actually stands and resolves the hand.
4. `Profile::record_attempt` stores the chosen and expected actions, and creates/updates a review card.
5. After a successful `deal` or `act`, the controller records a completed original round once and saves the profile. Finished games reject further actions, so rendering or rejected commands cannot count the round again.
6. egui paints the new table and the correction. The financial outcome does not alter the mistake record.

If the same hard 16 came **after a hit**, surrender is unavailable and the recommendation changes. This is why a single key such as `"16 vs 10"` would be an incorrect data model.

### Learn by making predictions, then running the code

1. Before opening `hand_value`, predict the totals and softness of A,A,9; A,6; and A,6,10. Trace how the borrowed card slice is evaluated without moving its cards.
2. Read the H17 strategy test and predict the answers for 11 vs ace, soft 18 vs 2, soft 19 vs 6, and 8,8 vs ace. Then run the strategy tests.
3. Trace an illegal action through `Game::act`. Identify the check that prevents it from partially changing the shoe or hand.
4. Follow a review through an error, a correct answer before it is due, and a correct answer after it is due. Predict which fields change before running the learning tests.
5. In a debugger, stop after `game.act` inside the controller. Inspect the stored pre-move `Situation` beside the changed hand. This makes the distinction between copied values and mutable ownership concrete.
6. Run `cargo doc --no-deps --open`, then browse the public library types alongside their source.

## Verification

```sh
cargo fmt --all --check
cargo check --locked --all-targets
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
```

The regression tests cover strategy chart boundaries and legal fallbacks, multiple aces, natural/split payouts, insurance, original-bet-only settlement, split limits, exact practice contexts, the 250-round boundary, scheduling transitions, analytics separation, six-deck card-counting boundaries and Hi-Lo values, database corruption, locking, and round-trip persistence.

Development verification exercised debug and optimized native Linux windows with real mouse/keyboard input: dealing, mistake feedback with the chosen move applied, the 249-to-250 assessment transition (including keeping the final correction visible), heatmaps at the minimum window size, a targeted hand, a 12-hand review plan, and persisted progress after closing. Engine smoke exercised 10,000 completed rounds and replayed 675 observed contexts as practice hands. The Apple Silicon target was checked with `cargo check --locked --target aarch64-apple-darwin`.

**Not verified in the Linux development environment:** actual macOS launching, VoiceOver behavior, `.app` bundling on macOS, or signing/notarization. Build and launch on your Mac using the commands above to verify the native platform integration.
