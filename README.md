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
The grid opens sorted by group, showing **market-listed blueprints only** with **Tech II and III hidden**.
Most blueprints that aren't market listed can't be obtained as originals, Tech II BPOs only come from
rare events or dev gifts, and Tech III only comes as reverse-engineered copies. The owned totals follow
these filters too. Untick **Hide Tech II/III** (or pick a tech level in the tech filter) to see them.
Each group has a header with a progress bar. Click a header to collapse or expand it, or use
**Expand all / Collapse all**. Collapsed groups are remembered between sessions.
Ship groups follow the in-game ship tree (frigates → destroyers → cruisers → battlecruisers →
battleships → capitals → industrial). Other groups are alphabetical. The ship order is a
hand-made list in `src/data.rs` (`SHIP_TREE_ORDER`), because the game data doesn't include the tree layout.

**Ignore** removes a blueprint from your completion count, for example BPC-only items
you don't count as part of the set.

## Syncing from EVE
**Characters › Log in with EVE** (or the button at the top right) opens EVE's official login page
in your browser. Pick a character and approve access. The tracker only asks to read blueprints.
After that it fills in your collection automatically: on start-up, and whenever you click **Sync**.
Add more characters the same way and their blueprints are combined.

- With two or more characters, a picker next to **Sync** shows **All characters** or just the ones you
  tick, without logging anyone out. Cards, totals and group headers then count only those characters'
  originals. Hand-ticked blueprints only appear under All characters, and ownership edits are made there.
- **Industry jobs:** research, copying and manufacturing jobs show on their card. A soft light sweeps
  across the card while a job is running. The ME or TE slot shows e.g. **7→9** with a progress bar,
  and the card says how long is left, or "ready to deliver". The side panel lists each job, and the
  status filter has an **In industry** option. Blueprints away in a research or copy job stay owned.
  This needs the `esi-industry.read_character_jobs.v1` permission. Characters that logged in before it
  was added just need to log in again.
- Only **originals** count. Copies are ignored.
- If you own several originals of one blueprint, the card shows the total and the **best** ME/TE.
  The side panel lists every original with its character and ME/TE.
- EVE refreshes blueprint data about once an hour, so a sync can lag slightly behind the game.
- Sync wins for anything EVE reports. Blueprints a previous sync marked as owned are marked missing
  once they're gone. Anything you ticked by hand, plus favourites, notes and ignores, is left alone.
- If any character fails to sync, nothing is changed, so a hiccup can't wipe blueprints.

The login uses EVE's PKCE flow for desktop apps, so no secret is stored in the app.
Login tokens are kept in `accounts.json` in your data folder. Remove a character via the
Characters menu, or revoke access at any time from the third-party applications page of your EVE account.

## Your data
Saved automatically in `%APPDATA%\EVE BPO Tracker\`: `collection.json` (your progress), `bpos.json`
(blueprint list), `settings.json` (collapsed groups), `accounts.json`, `sync_details.json` and `jobs.json`
(EVE logins, synced originals and industry jobs) and `icons\` (image cache from images.evetech.net). This is the same folder and
format the earlier Python version used, so existing progress carries over.
Use *File › Back up collection* occasionally. *Export to CSV* gives you a spreadsheet.

## Notes
- "Not on market" means the blueprint type has no market group in the SDE. That's usually faction,
  storyline, T2 or event items, many of which only exist as BPCs.
- Tech level, group and category come from the item the blueprint makes.
- The UI font is Inter, under the SIL Open Font License (`assets/fonts/OFL.txt`).

## Project layout
- `src/data.rs`: blueprint list builder, collection state, CSV export
- `src/esi.rs`: EVE login (SSO with PKCE) and blueprint sync
- `src/icons.rs`: background image loading and disk cache
- `src/app.rs`: the window (tabs, filters, card grid, detail panel)
- `src/theme.rs`: Solus colours and fonts
