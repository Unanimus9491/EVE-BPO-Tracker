# Slipway: Industry & Blueprint Suite

Industry and blueprint tools for EVE Online, starting with an AlecaFrame-style card grid for collecting
every Blueprint Original (BPO). Formerly *EVE BPO Tracker*.
It's a single Windows `.exe` written in Rust, and it doesn't need anything else installed.

## Getting the exe
Download `Slipway.exe` from the latest [GitHub Release](https://github.com/Unanimus9491/Slipway/releases/latest).

GitHub Actions only builds when a version tag is pushed, or when you start it by hand:

- **Release:** push a version tag (`git tag v0.5.5`, then `git push origin v0.5.5`). The exe is built,
  tested and attached to a GitHub Release, where the in-app updater finds it.
- **Test build without releasing:** open the repo's **Actions** tab, pick **Build Windows exe** and click
  **Run workflow**. When it finishes, download the **Slipway** artifact (a zip containing `Slipway.exe`).

Pushing to `main` doesn't trigger a build, so test locally first (see below).

### Building it yourself
Install Rust from https://rustup.rs, then run `cargo build --release` in this folder.
The exe ends up at `target/release/slipway.exe`.

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
The grid opens sorted by group, showing **market-listed blueprints only** with **unobtainable tiers hidden**.
Most blueprints that aren't market listed can't be obtained as originals, and Tech II, Tech III, Storyline,
Faction, Officer and Deadspace blueprints normally only exist as copies (BPCs), apart from rare event or
legacy BPOs. Any of those you own are always shown. The owned totals follow these filters too. Untick
**Hide unobtainable** to see the rest (the tech filter appears when it's unticked).
Each group is a card with a progress dial: the outer ring is owned, the inner ring fully researched.
**Click group** sets what clicking a group card does. **Open** shows only that group's blueprints, with
a Back button (or Esc / the mouse back button) to return. **Expand** opens it in place under a header,
with **Expand all / Collapse all**; expanded groups are remembered between sessions.
**File › Preferences…** (Ctrl+,) sets the card style: Rounded, Chamfered (default), Clipped or Machined.
Ship groups follow the in-game ship tree (frigates → destroyers → cruisers → battlecruisers →
battleships → capitals → industrial). Other groups are alphabetical. The ship order is a
hand-made list in `src/data.rs` (`SHIP_TREE_ORDER`), because the game data doesn't include the tree layout.

Outside ships and reactions, groups are sections of the in-game market tree, so related item groups share
one header (e.g. light, heavy, auto-targeting and assault missiles all sit under **Missiles**). Each card
still shows its own item group. Very broad market sections (`SPLIT_SECTIONS` in `src/data.rs`: Turrets &
Launchers, Rigs, Components) are split one level deeper.

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
  status filter has **Researching (ME/TE)** and **In industry (any job)** options. Blueprints away in a research or copy job stay owned.
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

## Updating
From 0.4.0 on, the app checks GitHub for a newer release each time it starts. When there is one,
a banner offers **Update and restart**: it downloads the new exe, swaps it in and reopens the app.
You can also use **Help › Check for updates…**. The previous exe is set aside as `….exe.old` and
deleted automatically once the new version has started, and nothing changes if the download fails.
A copy still named `EVE-BPO-Tracker.exe` installs the update as `Slipway.exe` in the same folder, so
re-pin it if you had the old name pinned to the taskbar or Start menu.

This needs the repo's releases to be public. Keep the exe in a folder you own, like Documents or
Desktop, rather than Program Files, so it's allowed to replace itself.

## Your data
Saved automatically in `%APPDATA%\Slipway\`: `collection.json` (your progress), `bpos.json`
(blueprint list), `settings.json` (expanded groups, the Click group setting and the card style), `accounts.json`, `sync_details.json` and `jobs.json`
(EVE logins, synced originals and industry jobs) and `icons\` (image cache from images.evetech.net). If you used the app before the rename, the old
`%APPDATA%\EVE BPO Tracker\` folder is moved here on first run, so existing progress carries over
(including from the earlier Python version).
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
- `src/updater.rs`: in-app updates from GitHub releases
- `src/app.rs`: the window (tabs, filters, card grid, detail panel)
- `src/theme.rs`: Solus colours and fonts

## License
The code is released under the [MIT License](LICENSE). Copyright © 2026 Unanimus.

This licence covers this project's own code only. The bundled Inter font is under the SIL Open Font License
(`assets/fonts/OFL.txt`). Game data and images the app downloads from CCP (the SDE, ESI and
images.evetech.net) stay subject to CCP's own terms.

EVE Online and the EVE logo are trademarks of CCP hf. All EVE-related data and imagery is the intellectual
property of CCP hf. This project is not affiliated with or endorsed by CCP hf.
