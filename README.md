# EVE BPO Collection Tracker

An AlecaFrame-style card grid for collecting every Blueprint Original (BPO) in EVE Online.

## Running it (Windows)
1. Install Python 3.10+ from python.org (tick "Add Python to PATH").
2. Double-click `run_tracker.bat`. The first time, it installs PySide6.
3. On first launch, click **Download**. The app fetches EVE's official Static Data Export
   (about 100 MB, one-off) and keeps only the blueprint list. Repeat via
   *File › Download / update game data* after big patches.

Manual: `pip install -r requirements.txt` then `python bpo_tracker.py`.

## Using it
Each card is one blueprint and has three slots:
- **BPO**: click to mark owned or missing. A badge shows quantity when you own more than one.
- **ME**: click for +1 (max 10). Shift-click for −1.
- **TE**: click for +2 (max 20). Shift-click for −2.

The card turns gold-brown when owned and gets a gold border at ME 10 / TE 20.
Click ☆ to favourite. Right-click for more (copies, max research, ignore).
The side panel edits the selected card, including notes.

**Ignore** removes a blueprint from your completion count, for example BPC-only
items you don't count as part of the set.

## Your data
Saved automatically in `%APPDATA%\EVE BPO Tracker\`: `collection.json` (your progress), `bpos.json`
(blueprint list) and `icons\` (image cache from images.evetech.net).
Use *File › Back up collection* occasionally. *Export to CSV* gives you a spreadsheet.

## Notes
- "Not on market" means the blueprint type has no market group in the SDE. That's usually faction,
  storyline, T2 or event items, many of which only exist as BPCs.
- Tech level, group and category come from the item the blueprint makes.
