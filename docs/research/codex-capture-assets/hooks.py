#!/usr/bin/env python3
"""Vetted, logging-only hook: record the provided event; don't infer child writes."""
import json, sys
from pathlib import Path
payload=json.load(sys.stdin)
with Path(sys.argv[1]).open('a') as f:
    f.write(json.dumps(payload)+'\n')
