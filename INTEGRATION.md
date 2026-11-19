# esp-hal integration branch

This branch is esp-hal `main` with changes that upstream has not merged yet. Each tag `integration-YYYY-MM-DD…` marks one state of the branch, and a project that builds on it must pin a tag or a commit, not the branch, because the branch is rebuilt on each new `main`.

## Base

esp-hal `main` at `f3bad48eb` (2026-11-18).

## Changes on top, in order

| Change | Commits | Upstream | Status |
|---|---|---|---|
| Code in PSRAM on the ESP32-S31 and the ESP32-C5 | 9 | [#6830](https://github.com/esp-rs/esp-hal/pull/6830) | draft |
