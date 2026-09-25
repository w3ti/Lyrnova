#!/usr/bin/python3
"""Protocol-v1 fixture, executed by the real runtime inside Bubblewrap."""
import json
from pathlib import Path
import sys


def send(frame):
    print(json.dumps(frame), flush=True)


for line in sys.stdin:
    frame = json.loads(line)
    if frame["type"] == "initialize":
        send({"type": "ready", "protocol_version": 1, "capabilities": ["tasks"]})
    elif frame["type"] == "shutdown":
        break
    elif frame["type"] == "request":
        if Path("/workspace/crash-plugin").exists():
            sys.exit(23)
        assert frame["operation"] == "tasks.list"
        send({"type": "response", "request_id": frame["request_id"], "capability": "tasks", "result": {
            "items": [{
                "id": "heartbeat", "label": "E2E heartbeat", "detail": "Writes a marker until canceled",
                "execution": {
                    "command": {"type": "shell", "shell": "sh", "script": "(printf 'task-child-started\\n'; while :; do printf x >> heartbeat; sleep 0.1; done) & wait"},
                    "cwd": None, "environment": {}, "access": "workspace_write", "network": False, "timeoutMs": 30000
                }
            }]
        }})
