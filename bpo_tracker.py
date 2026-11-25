"""EVE Online BPO Collection Tracker — an AlecaFrame-style card grid for blueprint originals.

Run:  python bpo_tracker.py
Needs: PySide6  (pip install PySide6)
"""
from __future__ import annotations

import csv
import json
import os
import shutil
import sys
from pathlib import Path

from PySide6.QtCore import (QAbstractListModel, QModelIndex, QObject, QRect, QRectF, QSize,
                            Qt, QThread, QTimer, QUrl, Signal)
from PySide6.QtGui import (QAction, QBrush, QColor, QDesktopServices, QFont, QFontMetrics,
                           QLinearGradient, QPainter, QPainterPath, QPen, QPixmap)
from PySide6.QtNetwork import QNetworkAccessManager, QNetworkReply, QNetworkRequest
from PySide6.QtWidgets import (QApplication, QButtonGroup, QCheckBox, QComboBox, QFileDialog,
                               QFormLayout, QFrame, QHBoxLayout, QLabel, QLineEdit, QListView,
                               QMainWindow, QMenu, QMessageBox, QProgressDialog, QPushButton,
                               QSpinBox, QStyle, QStyledItemDelegate, QVBoxLayout, QWidget,
                               QPlainTextEdit)

import sde_builder

APP_NAME = "EVE BPO Tracker"
ME_MAX, TE_MAX = 10, 20


def data_dir() -> Path:
    if sys.platform == "win32":
        base = Path(os.environ.get("APPDATA", Path.home()))
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    d = base / APP_NAME
    (d / "icons").mkdir(parents=True, exist_ok=True)
    return d


DATA = data_dir()
BPO_FILE = DATA / "bpos.json"
COLLECTION_FILE = DATA / "collection.json"


def atomic_write_json(path: Path, obj) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(obj, indent=1), encoding="utf-8")
    os.replace(tmp, path)


# ----------------------------------------------------------------------------- collection state
class Collection(QObject):
    changed = Signal(int)  # blueprint id

    DEFAULT = {"owned": False, "qty": 0, "me": 0, "te": 0, "fav": False, "ignored": False, "notes": ""}

    def __init__(self):
        super().__init__()
        self.state: dict[str, dict] = {}
        if COLLECTION_FILE.exists():
            try:
                self.state = json.loads(COLLECTION_FILE.read_text(encoding="utf-8"))
            except Exception:
                shutil.copy(COLLECTION_FILE, COLLECTION_FILE.with_suffix(".corrupt.json"))
        self._save_timer = QTimer(singleShot=True, interval=400)
        self._save_timer.timeout.connect(self.save)

    def get(self, bp_id: int) -> dict:
        return {**self.DEFAULT, **self.state.get(str(bp_id), {})}

    def update(self, bp_id: int, **kw) -> None:
        s = self.get(bp_id)
        s.update(kw)
        if "owned" in kw:
            if s["owned"] and s["qty"] < 1:
                s["qty"] = 1
            if not s["owned"]:
                s.update(qty=0, me=0, te=0)
        if "qty" in kw:
            s["qty"] = max(0, int(s["qty"]))
            s["owned"] = s["qty"] > 0
            if not s["owned"]:
                s.update(me=0, te=0)
        s["me"] = min(max(int(s["me"]), 0), ME_MAX)
        s["te"] = min(max(int(s["te"]), 0), TE_MAX)
        compact = {k: v for k, v in s.items() if v != self.DEFAULT[k]}
        if compact:
            self.state[str(bp_id)] = compact
        else:
            self.state.pop(str(bp_id), None)
        self.changed.emit(bp_id)
        self._save_timer.start()

    def save(self) -> None:
        atomic_write_json(COLLECTION_FILE, self.state)

    def replace_all(self, state: dict) -> None:
        self.state = state
        self.save()
        self.changed.emit(-1)


# ----------------------------------------------------------------------------- icon loading
class IconCache(QObject):
    """Loads item images from the EVE image server, cached on disk."""
    loaded = Signal(str)

    def __init__(self):
        super().__init__()
        self.mem: dict[str, QPixmap | None] = {}
        self.pending: set[str] = set()
        self.nam = QNetworkAccessManager(self)
        self.nam.finished.connect(self._done)

    @staticmethod
    def key_for(entry: dict) -> tuple[str, str]:
        if entry["tab"] == "Ships":
            return f"r{entry['product_id']}", f"https://images.evetech.net/types/{entry['product_id']}/render?size=128"
        if entry["kind"] == "reaction":
            return f"b{entry['id']}", f"https://images.evetech.net/types/{entry['id']}/bp?size=64"
        return f"i{entry['product_id']}", f"https://images.evetech.net/types/{entry['product_id']}/icon?size=64"

    def get(self, entry: dict) -> QPixmap | None:
        key, url = self.key_for(entry)
        if key in self.mem:
            return self.mem[key]
        path = DATA / "icons" / f"{key}.png"
        if path.exists():
            pm = QPixmap(str(path))
            self.mem[key] = pm if not pm.isNull() else None
            return self.mem[key]
        if key not in self.pending:
            self.pending.add(key)
            req = QNetworkRequest(QUrl(url))
            req.setHeader(QNetworkRequest.KnownHeaders.UserAgentHeader, sde_builder.USER_AGENT)
            req.setAttribute(QNetworkRequest.Attribute.User, key)
            self.nam.get(req)
        return None

    def _done(self, reply: QNetworkReply):
        key = reply.request().attribute(QNetworkRequest.Attribute.User)
        self.pending.discard(key)
        if reply.error() == QNetworkReply.NetworkError.NoError:
            data = reply.readAll()
            pm = QPixmap()
            if pm.loadFromData(data):
                pm.save(str(DATA / "icons" / f"{key}.png"), "PNG")
                self.mem[key] = pm
                self.loaded.emit(key)
        else:
            self.mem[key] = None  # don't retry this session
        reply.deleteLater()


# ----------------------------------------------------------------------------- model
class BpoModel(QAbstractListModel):
    EntryRole = Qt.ItemDataRole.UserRole + 1

    def __init__(self, entries: list[dict]):
        super().__init__()
        self.all = entries
        self.rows: list[dict] = list(entries)
        self.row_of: dict[int, int] = {}
        self._reindex()

    def _reindex(self):
        self.row_of = {e["id"]: i for i, e in enumerate(self.rows)}

    def rowCount(self, parent=QModelIndex()):
        return 0 if parent.isValid() else len(self.rows)

    def data(self, index, role=Qt.ItemDataRole.DisplayRole):
        if not index.isValid():
            return None
        e = self.rows[index.row()]
        if role == self.EntryRole:
            return e
        if role == Qt.ItemDataRole.DisplayRole:
            return e["name"]
        if role == Qt.ItemDataRole.ToolTipRole:
            return f"{e['name']}\n{e['product_name']} · {e['group']} · {e['tech']}\n" \
                   f"{'Market listed' if e['market'] else 'Not on market'}\n\n" \
                   "Click BPO slot: own / unown · Click ME/TE: +1 step (Shift: −1)\n" \
                   "Click ☆: favourite · Right-click: more"
        return None

    def set_rows(self, rows: list[dict]):
        self.beginResetModel()
        self.rows = rows
        self._reindex()
        self.endResetModel()

    def refresh_id(self, bp_id: int):
        r = self.row_of.get(bp_id)
        if r is not None:
            idx = self.index(r)
            self.dataChanged.emit(idx, idx)


# ----------------------------------------------------------------------------- card painting
C = {
    "window": QColor("#141a26"), "card_missing": QColor("#262c38"), "card_missing_b": QColor("#363e4d"),
    "card_owned_top": QColor("#4d4535"), "card_owned_bot": QColor("#3a3529"), "card_owned_b": QColor("#8a7a52"),
    "gold": QColor("#e2b84a"), "text": QColor("#ece8df"), "muted": QColor("#9aa3b2"),
    "slot": QColor("#1f2430"), "slot_b": QColor("#3a4150"),
    "slot_lit": QColor("#2f4a3a"), "slot_lit_b": QColor("#5d9a6d"),
    "slot_gold": QColor("#4a4026"), "green": QColor("#7ccf8a"), "red": QColor("#d06a6a"),
    "select": QColor("#7aa2ff"),
}
CARD_W, CARD_H, GAP = 300, 196, 10


def card_geometry(rect: QRect) -> dict[str, QRect]:
    r = rect.adjusted(GAP // 2, GAP // 2, -GAP // 2, -GAP // 2)
    x, y = r.x(), r.y()
    slot = 50
    sx = x + 116
    return {
        "card": r,
        "star": QRect(x + 10, y + 10, 20, 20),
        "name": QRect(x + 36, y + 8, r.width() - 46, 24),
        "image": QRect(x + 12, y + 44, 92, 92),
        "bpo": QRect(sx, y + 50, slot, slot),
        "me": QRect(sx + 1 * (slot + 8), y + 50, slot, slot),
        "te": QRect(sx + 2 * (slot + 8), y + 50, slot, slot),
        "group": QRect(sx, y + 110, r.width() - 128, 16),
        "tech": QRect(sx, y + 126, r.width() - 128, 16),
        "footer": QRect(x + 12, y + r.height() - 36, r.width() - 24, 30),
        "divider_y": QRect(x + 12, y + r.height() - 40, r.width() - 24, 1),
    }


class CardDelegate(QStyledItemDelegate):
    def __init__(self, coll: Collection, icons: IconCache, parent=None):
        super().__init__(parent)
        self.coll, self.icons = coll, icons
        self.f_name = QFont(); self.f_name.setPointSize(11); self.f_name.setBold(True)
        self.f_small = QFont(); self.f_small.setPointSize(8)
        self.f_slot = QFont(); self.f_slot.setPointSize(10); self.f_slot.setBold(True)
        self.f_badge = QFont(); self.f_badge.setPointSize(8); self.f_badge.setBold(True)
        self.f_foot = QFont(); self.f_foot.setPointSize(9); self.f_foot.setBold(True)

    def sizeHint(self, option, index):
        return QSize(CARD_W + GAP, CARD_H + GAP)

    # ---- painting helpers
    def _slot(self, p: QPainter, r: QRect, label: str, value_text: str, lit: bool, maxed: bool,
              frac: float | None = None, badge: str | None = None):
        path = QPainterPath(); path.addRoundedRect(QRectF(r), 9, 9)
        bg = C["slot_gold"] if maxed else (C["slot_lit"] if lit else C["slot"])
        bd = C["gold"] if maxed else (C["slot_lit_b"] if lit else C["slot_b"])
        p.fillPath(path, bg)
        p.setPen(QPen(bd, 1.4)); p.drawPath(path)
        p.setFont(self.f_small); p.setPen(C["muted"] if not lit else C["text"])
        p.drawText(r.adjusted(0, 4, 0, 0), Qt.AlignmentFlag.AlignHCenter | Qt.AlignmentFlag.AlignTop, label)
        p.setFont(self.f_slot); p.setPen(C["gold"] if maxed else (C["text"] if lit else C["muted"]))
        p.drawText(r.adjusted(0, 16, 0, -10), Qt.AlignmentFlag.AlignCenter, value_text)
        if frac is not None:
            bar = QRect(r.x() + 7, r.bottom() - 9, r.width() - 14, 4)
            p.fillRect(bar, QColor(0, 0, 0, 120))
            if frac > 0:
                p.fillRect(QRect(bar.x(), bar.y(), int(bar.width() * frac), bar.height()),
                           C["gold"] if maxed else C["green"])
        if badge:
            p.setFont(self.f_badge)
            w = QFontMetrics(self.f_badge).horizontalAdvance(badge) + 10
            br = QRect(r.right() - w + 5, r.y() - 6, w, 16)
            bp = QPainterPath(); bp.addRoundedRect(QRectF(br), 5, 5)
            p.fillPath(bp, QColor("#0d0f14")); p.setPen(QPen(C["slot_b"], 1)); p.drawPath(bp)
            p.setPen(C["text"]); p.drawText(br, Qt.AlignmentFlag.AlignCenter, badge)

    def paint(self, p: QPainter, option, index):
        e = index.data(BpoModel.EntryRole)
        s = self.coll.get(e["id"])
        g = card_geometry(option.rect)
        p.save()
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        if s["ignored"]:
            p.setOpacity(0.35)
        owned = s["owned"]
        researched = owned and s["me"] >= ME_MAX and s["te"] >= TE_MAX

        # card body
        card = QPainterPath(); card.addRoundedRect(QRectF(g["card"]), 10, 10)
        if owned:
            grad = QLinearGradient(g["card"].topLeft(), g["card"].bottomLeft())
            grad.setColorAt(0, C["card_owned_top"]); grad.setColorAt(1, C["card_owned_bot"])
            p.fillPath(card, QBrush(grad))
            border = C["gold"] if researched else C["card_owned_b"]
        else:
            p.fillPath(card, C["card_missing"]); border = C["card_missing_b"]
        if option.state & QStyle.StateFlag.State_Selected:
            border = C["select"]
        p.setPen(QPen(border, 2 if (researched or option.state & QStyle.StateFlag.State_Selected) else 1.2))
        p.drawPath(card)

        # header
        p.setFont(self.f_name)
        p.setPen(C["gold"] if s["fav"] else C["muted"])
        p.drawText(g["star"], Qt.AlignmentFlag.AlignCenter, "★" if s["fav"] else "☆")
        p.setPen(C["text"])
        name = e["name"].removesuffix(" Blueprint").removesuffix(" Reaction Formula")
        p.drawText(g["name"], Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft,
                   QFontMetrics(self.f_name).elidedText(name, Qt.TextElideMode.ElideRight, g["name"].width()))

        # image
        pm = self.icons.get(e)
        ir = g["image"]
        if pm:
            scaled = pm.scaled(ir.size(), Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation)
            p.drawPixmap(ir.x() + (ir.width() - scaled.width()) // 2, ir.y() + (ir.height() - scaled.height()) // 2, scaled)
        else:
            ph = QPainterPath(); ph.addRoundedRect(QRectF(ir), 8, 8)
            p.fillPath(ph, QColor(0, 0, 0, 60))
            p.setPen(C["muted"]); p.setFont(self.f_slot)
            initials = "".join(w[0] for w in name.split()[:2]).upper()
            p.drawText(ir, Qt.AlignmentFlag.AlignCenter, initials)

        # slots
        self._slot(p, g["bpo"], "BPO", "✓" if owned else "—", owned, False,
                   badge=str(s["qty"]) if s["qty"] > 1 else None)
        if e.get("researchable", True):
            self._slot(p, g["me"], "ME", f"{s['me']}", owned and s["me"] > 0, owned and s["me"] >= ME_MAX,
                       frac=s["me"] / ME_MAX if owned else 0)
            self._slot(p, g["te"], "TE", f"{s['te']}", owned and s["te"] > 0, owned and s["te"] >= TE_MAX,
                       frac=s["te"] / TE_MAX if owned else 0)

        p.setFont(self.f_small); p.setPen(C["muted"])
        fm = QFontMetrics(self.f_small)
        p.drawText(g["group"], Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter,
                   fm.elidedText(e["group"], Qt.TextElideMode.ElideRight, g["group"].width()))
        p.drawText(g["tech"], Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter,
                   e["tech"] + ("" if e["market"] else "  ·  not on market"))

        # footer
        p.fillRect(g["divider_y"], QColor(255, 255, 255, 28))
        fr = g["footer"]
        p.setFont(self.f_foot)
        if s["ignored"]:
            p.setPen(C["muted"]); text = "Ignored"
        elif researched:
            p.setPen(C["gold"]); text = "★ Fully researched"
        elif owned:
            p.setPen(C["green"]); text = "✓ Owned"
        else:
            p.setPen(C["red"]); text = "✗ Missing"
        p.drawText(fr, Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter, text)
        p.setPen(C["muted"]); p.setFont(self.f_small)
        p.drawText(fr, Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, e["tab"].rstrip("s") if e["tab"] != "Other" else e["category"])
        if s["notes"]:
            p.drawText(fr.adjusted(70, 0, 0, 0), Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, "✎ note")
        p.restore()

    def editorEvent(self, event, model, option, index):
        from PySide6.QtCore import QEvent
        if event.type() != QEvent.Type.MouseButtonRelease or event.button() != Qt.MouseButton.LeftButton:
            return False
        e = index.data(BpoModel.EntryRole)
        s = self.coll.get(e["id"])
        g = card_geometry(option.rect)
        pos = event.position().toPoint()
        shift = bool(event.modifiers() & Qt.KeyboardModifier.ShiftModifier)
        if g["star"].adjusted(-4, -4, 4, 4).contains(pos):
            self.coll.update(e["id"], fav=not s["fav"]); return False
        if g["bpo"].contains(pos):
            self.coll.update(e["id"], owned=not s["owned"]); return False
        if e.get("researchable", True) and s["owned"]:
            if g["me"].contains(pos):
                self.coll.update(e["id"], me=(s["me"] - 1) if shift else min(s["me"] + 1, ME_MAX)); return False
            if g["te"].contains(pos):
                self.coll.update(e["id"], te=(s["te"] - 2) if shift else min(s["te"] + 2, TE_MAX)); return False
        return False


# ----------------------------------------------------------------------------- detail panel
class DetailPanel(QFrame):
    def __init__(self, coll: Collection, icons: IconCache):
        super().__init__()
        self.coll, self.icons, self.entry = coll, icons, None
        self.setFixedWidth(290)
        self.setObjectName("detail")
        lay = QVBoxLayout(self)
        self.img = QLabel(alignment=Qt.AlignmentFlag.AlignCenter); self.img.setFixedHeight(140)
        self.title = QLabel(wordWrap=True); self.title.setObjectName("detailTitle")
        self.sub = QLabel(wordWrap=True); self.sub.setObjectName("muted")
        lay.addWidget(self.img); lay.addWidget(self.title); lay.addWidget(self.sub)
        form = QFormLayout()
        self.owned = QCheckBox("Owned")
        self.qty = QSpinBox(maximum=999)
        self.me = QSpinBox(maximum=ME_MAX)
        self.te = QSpinBox(maximum=TE_MAX, singleStep=2)
        self.fav = QCheckBox("Favourite")
        self.ignored = QCheckBox("Ignore (exclude from collection)")
        self.maxbtn = QPushButton("Max research (ME 10 / TE 20)")
        form.addRow(self.owned); form.addRow("Quantity", self.qty)
        form.addRow("Material Eff.", self.me); form.addRow("Time Eff.", self.te)
        form.addRow(self.maxbtn); form.addRow(self.fav); form.addRow(self.ignored)
        lay.addLayout(form)
        lay.addWidget(QLabel("Notes"))
        self.notes = QPlainTextEdit(); self.notes.setPlaceholderText("Where it is, what you paid, plans…")
        lay.addWidget(self.notes, 1)
        self._loading = False
        self.owned.toggled.connect(lambda v: self._set(owned=v))
        self.qty.valueChanged.connect(lambda v: self._set(qty=v))
        self.me.valueChanged.connect(lambda v: self._set(me=v))
        self.te.valueChanged.connect(lambda v: self._set(te=v))
        self.fav.toggled.connect(lambda v: self._set(fav=v))
        self.ignored.toggled.connect(lambda v: self._set(ignored=v))
        self.maxbtn.clicked.connect(lambda: self._set(owned=True, me=ME_MAX, te=TE_MAX))
        self.notes.textChanged.connect(lambda: self._set(notes=self.notes.toPlainText()))
        coll.changed.connect(self._on_changed)
        icons.loaded.connect(lambda _k: self._refresh_image())
        self.show_entry(None)

    def _set(self, **kw):
        if self._loading or not self.entry:
            return
        self.coll.update(self.entry["id"], **kw)

    def _on_changed(self, bp_id):
        if self.entry and bp_id in (-1, self.entry["id"]):
            self.show_entry(self.entry)

    def _refresh_image(self):
        if self.entry:
            pm = self.icons.get(self.entry)
            if pm:
                self.img.setPixmap(pm.scaled(128, 128, Qt.AspectRatioMode.KeepAspectRatio,
                                             Qt.TransformationMode.SmoothTransformation))

    def show_entry(self, e):
        self.entry = e
        self._loading = True
        enabled = e is not None
        for w in (self.owned, self.qty, self.me, self.te, self.fav, self.ignored, self.maxbtn, self.notes):
            w.setEnabled(enabled)
        if not e:
            self.title.setText("Select a blueprint"); self.sub.setText("Click a card to edit it here.")
            self.img.clear(); self._loading = False; return
        s = self.coll.get(e["id"])
        self.title.setText(e["name"])
        self.sub.setText(f"Makes: {e['product_name']}\n{e['category']} › {e['group']}\n{e['tech']} · "
                         f"{'Market listed' if e['market'] else 'Not on market'}\nType ID {e['id']}")
        self.owned.setChecked(s["owned"]); self.qty.setValue(s["qty"])
        self.me.setValue(s["me"]); self.te.setValue(s["te"])
        for w in (self.me, self.te, self.maxbtn):
            w.setEnabled(s["owned"] and e.get("researchable", True))
        self.fav.setChecked(s["fav"]); self.ignored.setChecked(s["ignored"])
        if self.notes.toPlainText() != s["notes"]:
            self.notes.setPlainText(s["notes"])
        self.img.clear(); self._refresh_image()
        self._loading = False


# ----------------------------------------------------------------------------- SDE worker
class BuildWorker(QThread):
    progress = Signal(str, int, int)
    finished_ok = Signal(list)
    failed = Signal(str)

    def __init__(self, zip_path: str | None = None):
        super().__init__()
        self.zip_path = zip_path

    def run(self):
        try:
            cb = lambda m, d, t: self.progress.emit(m, d, t)
            data = (sde_builder.build_dataset(self.zip_path, cb) if self.zip_path
                    else sde_builder.download_and_build(cb))
            self.finished_ok.emit(data)
        except Exception as ex:  # noqa: BLE001
            self.failed.emit(f"{type(ex).__name__}: {ex}")


# ----------------------------------------------------------------------------- main window
STYLE = """
QWidget { background: #141a26; color: #ece8df; font-size: 10pt; }
QLabel#title { font-size: 20pt; font-weight: bold; }
QLabel#muted { color: #9aa3b2; }
QLabel#stats { color: #c9c2b2; font-size: 10pt; }
QLabel#detailTitle { font-size: 13pt; font-weight: bold; }
QPushButton { background: #222938; border: 1px solid #343c4c; border-radius: 6px; padding: 6px 12px; }
QPushButton:hover { background: #2b3345; }
QPushButton:checked { background: #3a3f6a; border-color: #6c74c9; }
QLineEdit, QComboBox, QSpinBox, QPlainTextEdit { background: #1c2230; border: 1px solid #343c4c;
    border-radius: 6px; padding: 5px; }
QListView { background: #141a26; border: none; }
QFrame#detail { background: #1a2030; border-left: 1px solid #2a3142; }
QProgressBar { border: 1px solid #343c4c; border-radius: 4px; text-align: center; }
QProgressBar::chunk { background: #e2b84a; }
QMenu { background: #1c2230; border: 1px solid #343c4c; }
QMenu::item:selected { background: #3a3f6a; }
QCheckBox::indicator { width: 14px; height: 14px; border: 1px solid #5a6478; border-radius: 3px; background: #1c2230; }
QCheckBox::indicator:checked { background: #e2b84a; border-color: #e2b84a; }
QLabel, QCheckBox { background: transparent; }
QScrollBar:vertical { background: #141a26; width: 12px; margin: 0; }
QScrollBar::handle:vertical { background: #343c4c; border-radius: 5px; min-height: 40px; }
QScrollBar::handle:vertical:hover { background: #4a5468; }
QScrollBar::add-line, QScrollBar::sub-line { height: 0; }
QScrollBar::add-page, QScrollBar::sub-page { background: none; }
"""


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(APP_NAME)
        self.resize(1500, 900)
        self.coll = Collection()
        self.icons = IconCache()
        self.entries: list[dict] = []
        self.tab = "All"
        self.worker = None

        central = QWidget(); self.setCentralWidget(central)
        outer = QHBoxLayout(central); outer.setContentsMargins(0, 0, 0, 0); outer.setSpacing(0)
        main = QWidget(); outer.addWidget(main, 1)
        v = QVBoxLayout(main); v.setContentsMargins(16, 12, 16, 8)

        head = QHBoxLayout()
        t = QLabel("Blueprint Collection"); t.setObjectName("title")
        self.stats = QLabel(); self.stats.setObjectName("stats")
        head.addWidget(t); head.addSpacing(20); head.addWidget(self.stats, 1)
        v.addLayout(head)

        # tabs
        tabs = QHBoxLayout(); self.tab_group = QButtonGroup(self); self.tab_buttons = {}
        for name in sde_builder.TABS:
            b = QPushButton(name); b.setCheckable(True)
            b.clicked.connect(lambda _=False, n=name: self.set_tab(n))
            self.tab_group.addButton(b); tabs.addWidget(b); self.tab_buttons[name] = b
        self.tab_buttons["All"].setChecked(True)
        tabs.addStretch(1)
        self.search = QLineEdit(placeholderText="🔍  Search blueprint or product…"); self.search.setMinimumWidth(260)
        tabs.addWidget(self.search)
        v.addLayout(tabs)

        # filters
        f = QHBoxLayout()
        self.f_status = QComboBox(); self.f_status.addItems(
            ["Any status", "Missing", "Owned", "Owned – not fully researched", "Fully researched",
             "Duplicates (qty > 1)", "Favourites", "Ignored"])
        self.f_tech = QComboBox()
        self.f_group = QComboBox(); self.f_group.setMinimumWidth(220)
        self.f_market = QComboBox(); self.f_market.addItems(["Market + non-market", "Market listed only", "Not on market only"])
        self.f_sort = QComboBox(); self.f_sort.addItems(["Sort: Name", "Sort: Group", "Sort: Missing first", "Sort: Owned first"])
        self.f_hide_ignored = QCheckBox("Hide ignored"); self.f_hide_ignored.setChecked(True)
        for w in (self.f_status, self.f_tech, self.f_group, self.f_market, self.f_sort):
            f.addWidget(w)
        f.addWidget(self.f_hide_ignored); f.addStretch(1)
        self.shown = QLabel(); self.shown.setObjectName("muted"); f.addWidget(self.shown)
        v.addLayout(f)

        # grid
        self.model = BpoModel([])
        self.view = QListView()
        self.view.setViewMode(QListView.ViewMode.IconMode)
        self.view.setResizeMode(QListView.ResizeMode.Adjust)
        self.view.setMovement(QListView.Movement.Static)
        self.view.setUniformItemSizes(True)
        self.view.setGridSize(QSize(CARD_W + GAP, CARD_H + GAP))
        self.view.setSpacing(0)
        self.view.setMouseTracking(True)
        self.view.setVerticalScrollMode(QListView.ScrollMode.ScrollPerPixel)
        self.view.verticalScrollBar().setSingleStep(30)
        self.view.setModel(self.model)
        self.view.setItemDelegate(CardDelegate(self.coll, self.icons, self.view))
        self.view.setContextMenuPolicy(Qt.ContextMenuPolicy.CustomContextMenu)
        self.view.customContextMenuRequested.connect(self.context_menu)
        self.view.selectionModel().currentChanged.connect(
            lambda cur, _prev: self.detail.show_entry(cur.data(BpoModel.EntryRole) if cur.isValid() else None))
        v.addWidget(self.view, 1)

        self.detail = DetailPanel(self.coll, self.icons)
        outer.addWidget(self.detail)

        self.empty = QWidget()
        self._build_menu()

        self._refilter_timer = QTimer(singleShot=True, interval=150)
        self._refilter_timer.timeout.connect(self.apply_filters)
        self.search.textChanged.connect(lambda _: self._refilter_timer.start())
        for w in (self.f_status, self.f_tech, self.f_group, self.f_market, self.f_sort):
            w.currentIndexChanged.connect(lambda _=0: self._refilter_timer.start())
        self.f_hide_ignored.toggled.connect(lambda _: self._refilter_timer.start())
        self.coll.changed.connect(self._on_coll_changed)
        self.icons.loaded.connect(lambda _k: self.view.viewport().update())

        self.load_dataset()

    # ---------------- menu
    def _build_menu(self):
        mb = self.menuBar()
        m = mb.addMenu("&File")
        for text, fn in [("Download / update game data", self.update_data),
                         ("Build from a local SDE zip…", self.build_from_zip),
                         (None, None),
                         ("Export collection to CSV…", self.export_csv),
                         ("Back up collection…", self.backup),
                         ("Restore collection from backup…", self.restore),
                         (None, None),
                         ("Open data folder", lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(DATA)))),
                         ("Quit", self.close)]:
            if text is None:
                m.addSeparator()
            else:
                a = QAction(text, self); a.triggered.connect(fn); m.addAction(a)

    # ---------------- data
    def load_dataset(self):
        if BPO_FILE.exists():
            self.entries = json.loads(BPO_FILE.read_text(encoding="utf-8"))
            self.model.all = self.entries
            self._populate_filters()
            self.apply_filters()
        else:
            QTimer.singleShot(200, self._first_run)

    def _first_run(self):
        box = QMessageBox(self)
        box.setWindowTitle("Game data needed")
        box.setText("The tracker needs the list of blueprints from EVE's official Static Data Export.\n\n"
                    "Download it now? (One-off, roughly 100 MB; only the blueprint list is kept.)")
        dl = box.addButton("Download", QMessageBox.ButtonRole.AcceptRole)
        local = box.addButton("Use a local SDE zip…", QMessageBox.ButtonRole.ActionRole)
        box.addButton("Later", QMessageBox.ButtonRole.RejectRole)
        box.exec()
        if box.clickedButton() is dl:
            self.update_data()
        elif box.clickedButton() is local:
            self.build_from_zip()

    def update_data(self):
        self._run_worker(BuildWorker())

    def build_from_zip(self):
        path, _ = QFileDialog.getOpenFileName(self, "Choose SDE JSONL zip", "", "Zip files (*.zip)")
        if path:
            self._run_worker(BuildWorker(path))

    def _run_worker(self, worker: BuildWorker):
        if self.worker and self.worker.isRunning():
            return
        self.worker = worker
        dlg = QProgressDialog("Starting…", None, 0, 0, self)
        dlg.setWindowTitle("Updating game data"); dlg.setMinimumWidth(420)
        dlg.setWindowModality(Qt.WindowModality.WindowModal); dlg.show()

        def prog(msg, done, total):
            dlg.setLabelText(msg + (f"  {done / 1e6:.1f} / {total / 1e6:.1f} MB" if total > 100 else ""))
            if total > 100:
                dlg.setMaximum(1000); dlg.setValue(int(done / total * 1000))
            else:
                dlg.setMaximum(max(total, 1)); dlg.setValue(done)

        def ok(data):
            dlg.close()
            atomic_write_json(BPO_FILE, data)
            self.load_dataset()
            QMessageBox.information(self, APP_NAME, f"Loaded {len(data):,} blueprints.")

        def bad(msg):
            dlg.close()
            QMessageBox.warning(self, APP_NAME, "Couldn't build the blueprint list:\n\n" + msg +
                                "\n\nYou can download the JSONL zip yourself from developers.eveonline.com "
                                "(Static Data) and use File › Build from a local SDE zip.")

        worker.progress.connect(prog); worker.finished_ok.connect(ok); worker.failed.connect(bad)
        worker.start()

    # ---------------- filtering
    def set_tab(self, name):
        self.tab = name
        self._populate_groups()
        self.apply_filters()

    def _populate_filters(self):
        self.f_tech.blockSignals(True)
        cur = self.f_tech.currentText()
        self.f_tech.clear(); self.f_tech.addItem("Any tech")
        self.f_tech.addItems(sorted({e["tech"] for e in self.entries}))
        if cur: self.f_tech.setCurrentText(cur)
        self.f_tech.blockSignals(False)
        self._populate_groups()

    def _populate_groups(self):
        self.f_group.blockSignals(True)
        self.f_group.clear(); self.f_group.addItem("Any group")
        pool = self.entries if self.tab == "All" else [e for e in self.entries if e["tab"] == self.tab]
        self.f_group.addItems(sorted({e["group"] for e in pool}))
        self.f_group.blockSignals(False)

    def _matches(self, e, q, status, tech, group, market, hide_ignored):
        if self.tab != "All" and e["tab"] != self.tab:
            return False
        if q and q not in e["name"].lower() and q not in e["product_name"].lower() and q not in e["group"].lower():
            return False
        if tech and e["tech"] != tech: return False
        if group and e["group"] != group: return False
        if market == 1 and not e["market"]: return False
        if market == 2 and e["market"]: return False
        s = self.coll.get(e["id"])
        if hide_ignored and s["ignored"] and status != 7: return False
        full = s["owned"] and s["me"] >= ME_MAX and s["te"] >= TE_MAX
        return [True, not s["owned"], s["owned"], s["owned"] and not full, full,
                s["qty"] > 1, s["fav"], s["ignored"]][status]

    def apply_filters(self):
        q = self.search.text().strip().lower()
        status = self.f_status.currentIndex()
        tech = self.f_tech.currentText() if self.f_tech.currentIndex() > 0 else None
        group = self.f_group.currentText() if self.f_group.currentIndex() > 0 else None
        market = self.f_market.currentIndex()
        hide = self.f_hide_ignored.isChecked()
        rows = [e for e in self.entries if self._matches(e, q, status, tech, group, market, hide)]
        sort = self.f_sort.currentIndex()
        if sort == 1:
            rows.sort(key=lambda e: (e["group"].lower(), e["name"].lower()))
        elif sort in (2, 3):
            rows.sort(key=lambda e: (self.coll.get(e["id"])["owned"] == (sort == 2), e["name"].lower()))
        cur = self.view.currentIndex().data(BpoModel.EntryRole) if self.view.currentIndex().isValid() else None
        self.model.set_rows(rows)
        if cur and cur["id"] in self.model.row_of:
            self.view.setCurrentIndex(self.model.index(self.model.row_of[cur["id"]]))
        self.shown.setText(f"Showing {len(rows):,}")
        self.update_stats()

    def update_stats(self):
        pool = [e for e in self.entries if self.tab == "All" or e["tab"] == self.tab]
        counted = [e for e in pool if not self.coll.get(e["id"])["ignored"]]
        owned = [e for e in counted if self.coll.get(e["id"])["owned"]]
        full = [e for e in owned if self.coll.get(e["id"])["me"] >= ME_MAX and self.coll.get(e["id"])["te"] >= TE_MAX]
        n = len(counted) or 1
        label = "all categories" if self.tab == "All" else self.tab
        self.stats.setText(f"{label}:  <b>{len(owned):,}</b> / {len(counted):,} owned ({len(owned) / n:.1%})"
                           f"  ·  <span style='color:#e2b84a'>{len(full):,} fully researched</span>")

    def _on_coll_changed(self, bp_id):
        if bp_id == -1:
            self.apply_filters(); return
        self.model.refresh_id(bp_id)
        self.update_stats()

    # ---------------- context menu
    def context_menu(self, pos):
        idx = self.view.indexAt(pos)
        if not idx.isValid():
            return
        e = idx.data(BpoModel.EntryRole); s = self.coll.get(e["id"]); bid = e["id"]
        m = QMenu(self)
        m.addAction("Mark missing" if s["owned"] else "Mark owned", lambda: self.coll.update(bid, owned=not s["owned"]))
        m.addAction("Add another copy (qty +1)", lambda: self.coll.update(bid, qty=s["qty"] + 1))
        if s["qty"] > 1:
            m.addAction("Remove a copy (qty −1)", lambda: self.coll.update(bid, qty=s["qty"] - 1))
        if s["owned"] and e.get("researchable", True):
            m.addAction("Set fully researched (ME 10 / TE 20)", lambda: self.coll.update(bid, me=ME_MAX, te=TE_MAX))
            m.addAction("Reset research", lambda: self.coll.update(bid, me=0, te=0))
        m.addSeparator()
        m.addAction("Unfavourite" if s["fav"] else "Favourite", lambda: self.coll.update(bid, fav=not s["fav"]))
        m.addAction("Un-ignore" if s["ignored"] else "Ignore (not part of my collection)",
                    lambda: self.coll.update(bid, ignored=not s["ignored"]))
        m.addSeparator()
        m.addAction("Copy name", lambda: QApplication.clipboard().setText(e["name"]))
        m.exec(self.view.viewport().mapToGlobal(pos))

    # ---------------- import / export
    def export_csv(self):
        path, _ = QFileDialog.getSaveFileName(self, "Export CSV", "bpo_collection.csv", "CSV (*.csv)")
        if not path:
            return
        with open(path, "w", newline="", encoding="utf-8") as fh:
            w = csv.writer(fh)
            w.writerow(["Blueprint", "Type ID", "Product", "Tab", "Group", "Tech", "Market listed",
                        "Owned", "Quantity", "ME", "TE", "Favourite", "Ignored", "Notes"])
            for e in self.entries:
                s = self.coll.get(e["id"])
                w.writerow([e["name"], e["id"], e["product_name"], e["tab"], e["group"], e["tech"],
                            e["market"], s["owned"], s["qty"], s["me"], s["te"], s["fav"], s["ignored"], s["notes"]])
        QMessageBox.information(self, APP_NAME, f"Exported to {path}")

    def backup(self):
        path, _ = QFileDialog.getSaveFileName(self, "Back up collection", "bpo_collection_backup.json", "JSON (*.json)")
        if path:
            self.coll.save(); shutil.copy(COLLECTION_FILE, path)

    def restore(self):
        path, _ = QFileDialog.getOpenFileName(self, "Restore collection", "", "JSON (*.json)")
        if not path:
            return
        try:
            state = json.loads(Path(path).read_text(encoding="utf-8"))
            assert isinstance(state, dict)
        except Exception as ex:  # noqa: BLE001
            QMessageBox.warning(self, APP_NAME, f"Not a valid backup: {ex}"); return
        if QMessageBox.question(self, APP_NAME, "Replace your current collection with this backup?") \
                == QMessageBox.StandardButton.Yes:
            self.coll.replace_all(state)

    def closeEvent(self, ev):
        self.coll.save()
        super().closeEvent(ev)


def main():
    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    app.setStyle("Fusion")
    app.setStyleSheet(STYLE)
    w = MainWindow(); w.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
