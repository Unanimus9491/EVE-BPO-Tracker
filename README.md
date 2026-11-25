# EVE BPO Collection Tracker

An AlecaFrame-style card grid for collecting every Blueprint Original (BPO) in EVE Online.
It's a single Windows `.exe` written in Rust, and it doesn't need anything else installed.

## Getting the exe
GitHub Actions builds the exe every time you push to `main`:

1. Open the repo's **Actions** tab and pick the latest **Build Windows exe** run.
2. Download the **EVE-BPO-Tracker** artifact. It's a zip that contains `EVE-BPO-Tracker.exe`.

To publish a proper release, push a version tag (`git tag v0.2.0`, then `git push --tags`).
The exe is then attached to a GitHub Release.

### Building it yourself
Install Rust from https://rustup.rs, then run `cargo build --release` in this folder.
The exe ends up at `target/release/eve-bpo-tracker.exe`.

## First run
Click **Download**. The app fetches EVE's official Static Data Export (about 100 MB, one-off)
and keeps only the blueprint list. Repeat via *File › Download / update game data* after big patches.

## Using it
Each card is one blueprint and has three slots:
- **BPO**: click to mark owned or missing. A badge shows quantity when you own more than one.
- **ME**: click for +1 (max 10). Shift-click for −1.
- **TE**: click for +2 (max 20). Shift-click for −2.

The card lights up in pale gold when owned and gets a Solus Gold border at ME 10 / TE 20.
Click ☆ to favourite. Right-click for more (copies, max research, ignore).
The side panel edits the selected card, including notes.
**Sort: Group** splits the grid into groups, each with a header and a progress bar.

**Ignore** removes a blueprint from your completion count, for example BPC-only items
you don't count as part of the set.

## Your data
Saved automatically in `%APPDATA%\EVE BPO Tracker\`: `collection.json` (your progress), `bpos.json`
(blueprint list) and `icons\` (image cache from images.evetech.net). This is the same folder and
format the earlier Python version used, so existing progress carries over.
Use *File › Back up collection* occasionally. *Export to CSV* gives you a spreadsheet.

## Notes
- "Not on market" means the blueprint type has no market group in the SDE. That's usually faction,
  storyline, T2 or event items, many of which only exist as BPCs.
- Tech level, group and category come from the item the blueprint makes.
- The UI font is Inter, under the SIL Open Font License (`assets/fonts/OFL.txt`).

## Project layout
- `src/data.rs`: blueprint list builder, collection state, CSV export
- `src/icons.rs`: background image loading and disk cache
- `src/app.rs`: the window (tabs, filters, card grid, detail panel)
- `src/theme.rs`: Solus colours and fonts
