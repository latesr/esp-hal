# esp-hal integration branch

This branch is esp-hal `main` with changes that upstream has not merged yet. Each tag `integration-YYYY-MM-DD…` marks one state of the branch, and a project that builds on it must pin a tag or a commit, not the branch, because the branch is rebuilt on each new `main`.

## Base

esp-hal `main` at `34770b1e3` (2026-11-24).

## Changes on top, in order

| Change | Commits | Upstream | Status |
|---|---|---|---|
| Let the PSRAM controller code run before code is in PSRAM | 1 | [#6918](https://github.com/esp-rs/esp-hal/pull/6918) | open |
| Run code from PSRAM on the ESP32-S31 | 3 | [#6919](https://github.com/esp-rs/esp-hal/pull/6919) | open |
| Place zeroed statics in PSRAM with code in PSRAM | 1 | [#6920](https://github.com/esp-rs/esp-hal/pull/6920) | open |
| Let code in PSRAM run during flash operations | 2 | [#6921](https://github.com/esp-rs/esp-hal/pull/6921) | open |
| Code in PSRAM on the ESP32-C5 | 3 | not posted | in review |
