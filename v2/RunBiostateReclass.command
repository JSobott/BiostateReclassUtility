#!/bin/bash
# BiostateReclassUtility v2 — Single-click macOS launcher
# Double-click this file in Finder to start the app.
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure Node.js and Python 3.12 are on PATH
export PATH="$HOME/local/node/bin:$HOME/local/python312/bin:$PATH"

echo "========================================="
echo "  BiostateReclassUtility v2"
echo "  QBO Classification with Claude Opus 4.6"
echo "========================================="
echo ""

# 1. Kill any existing servers on port 3030 and 5173
echo "[1/6] Stopping existing servers..."
lsof -ti:3030 | xargs kill -9 2>/dev/null || true
lsof -ti:5173 | xargs kill -9 2>/dev/null || true
sleep 1

# 2. Python venv setup
echo "[2/6] Setting up Python environment..."
if [ ! -d "backend/.venv" ]; then
    echo "       Creating virtual environment..."
    python3.12 -m venv backend/.venv
fi
source backend/.venv/bin/activate
pip install -q -r backend/requirements.txt 2>&1 | tail -1

# 3. Start FastAPI backend
echo "[3/6] Starting backend on port 3030..."
cd backend
python -m uvicorn app.main:app --host 127.0.0.1 --port 3030 --log-level info &
BACKEND_PID=$!
cd ..

# 4. Frontend dependencies
echo "[4/6] Setting up frontend..."
if [ ! -d "frontend/node_modules" ]; then
    echo "       Installing npm dependencies..."
    (cd frontend && npm install --silent)
fi

# 5. Start Vite dev server
echo "[5/6] Starting frontend dev server..."
(cd frontend && npx vite --host 127.0.0.1 --port 5173) &
FRONTEND_PID=$!

# 6. Wait for servers and open Chrome
echo "[6/6] Opening Chrome..."
sleep 3

VITE_URL="http://127.0.0.1:5173"

osascript <<APPLESCRIPT
tell application "Google Chrome"
    activate
    set found to false
    repeat with w in windows
        set tabIndex to 0
        repeat with t in tabs of w
            set tabIndex to tabIndex + 1
            if URL of t starts with "http://127.0.0.1:5173" or URL of t starts with "http://localhost:5173" then
                set active tab index of w to tabIndex
                set URL of t to "$VITE_URL"
                set index of w to 1
                set found to true
                exit repeat
            end if
        end repeat
        if found then exit repeat
    end repeat
    if not found then
        if (count of windows) = 0 then
            make new window
        end if
        tell window 1 to make new tab with properties {URL:"$VITE_URL"}
    end if
end tell
APPLESCRIPT

echo ""
echo "App running at $VITE_URL"
echo "Backend API at http://127.0.0.1:3030"
echo ""
echo "Press Ctrl+C to stop all servers..."

# Cleanup on exit
cleanup() {
    echo ""
    echo "Shutting down..."
    kill $BACKEND_PID 2>/dev/null || true
    kill $FRONTEND_PID 2>/dev/null || true
    wait 2>/dev/null
    echo "Done."
}
trap cleanup EXIT INT TERM

wait
