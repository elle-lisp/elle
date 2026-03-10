#!/bin/bash
git add -A
git commit -m "Remove temporary scripts" || true
git push origin refactor-work
