@echo off
cd /d "%~dp0"
python -m pip install -q -r requirements.txt
python bpo_tracker.py
pause
