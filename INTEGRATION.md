# esp-hal integration branch

This branch is esp-hal `main` with changes that upstream has not merged yet. Each tag `integration-YYYY-MM-DD…` marks one state of the branch, and a project that builds on it must pin a tag or a commit, not the branch, because the branch is rebuilt on each new `main`.

## Base

esp-hal `main` at `c54d6d21d` (2026-11-11).

## Changes on top, in order

| Change | Commits | Upstream | Status |
|---|---|---|---|
| `pmu_sleep_start` and `pmu_sleep_finish` on the ESP32-S31 | 3 | [#6717](https://github.com/esp-rs/esp-hal/pull/6717) | submitted |
| `checked` in place of `cache_suspend_safe` in #6717's code | 1 | none | local only; dropped when #6717 uses `checked` |
| qa `sleep-checks` writes a new PSRAM pattern for each check | 1 | [#6806](https://github.com/esp-rs/esp-hal/pull/6806) | submitted |
| Code in PSRAM on the ESP32-S31 and the ESP32-C5 | 9 | draft pull request, not opened yet | waits for #6717 and #6806 |
