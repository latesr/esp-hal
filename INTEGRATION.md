# esp-hal integration branch

This branch is esp-hal `main` with changes that upstream has not merged yet. Each tag `integration-YYYY-MM-DD…` marks one state of the branch, and a project that builds on it must pin a tag or a commit, not the branch, because the branch is rebuilt on each new `main`.

## Base

esp-hal `main` at `3ee7ec0f1` (2026-11-13).

## Changes on top, in order

| Change | Commits | Upstream | Status |
|---|---|---|---|
| Code in PSRAM on the ESP32-S31 and the ESP32-C5. The first commit carries the ESP32-S31 sleep changes of [#6717](https://github.com/esp-rs/esp-hal/pull/6717), on the sleep state of #6813 | 10 | [#6830](https://github.com/esp-rs/esp-hal/pull/6830) | draft |
