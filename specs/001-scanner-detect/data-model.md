# Data Model: Scanner Detection and Capability Listing

**Feature**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)

All entities are in-memory Rust types; nothing is persisted. Field names below are the JSON
names used in [contracts/output.schema.json](contracts/output.schema.json).

## DeviceInfo

A scanner found by the backend (spec entity "Scanner Device").

| Field | Type | Rules |
|-------|------|-------|
| `name` | string | SANE device name, e.g. `pieusb:libusb:001:004`. Non-empty; used with `--device`. |
| `vendor` | string | As reported by the driver, e.g. `Reflecta`. May be empty. |
| `model` | string | As reported, e.g. `RPS 10M`. May be empty. |
| `kind` | string | SANE device type, e.g. `film scanner`. (`type` is a Rust keyword.) |

Only devices from the configured backend are ever listed (FR-002).

## Capability

One setting the scanner exposes (spec entity "Capability").

| Field | Type | Rules |
|-------|------|-------|
| `index` | integer | SANE option number, ≥ 1 (option 0 is the count and is not listed). |
| `name` | string | Machine name, e.g. `resolution`. Empty for groups. |
| `title` | string | Short human title. |
| `description` | string | Longer help text. May be empty. |
| `value_type` | ValueType | |
| `unit` | Unit | |
| `size` | integer | Bytes. For bool/int/fixed, `size / 4` = number of values. |
| `active` | bool | False when SANE marks the option `INACTIVE`. |
| `settable` | bool | True when SANE marks it `SOFT_SELECT` (software can set it). |
| `automatic` | bool | SANE `AUTOMATIC` flag. |
| `advanced` | bool | SANE `ADVANCED` flag. |
| `constraint` | Constraint | |
| `value` | Value or null | Null when the option is a button or group, inactive, or unreadable. |

**Validation**: a `value` is read only when `active` is true and `value_type` is not `button`
or `group`. Fixed values are converted with `raw / 65536.0`. Inactive options are listed,
never hidden (spec US2 scenario 3).

## ValueType

`bool` | `int` | `fixed` | `string` | `button` | `group`, mapped from `SANE_TYPE_*` 0–5. An
unknown number is an internal error naming the option.

## Unit

`none` | `pixel` | `bit` | `mm` | `dpi` | `percent` | `microsecond`, mapped from
`SANE_UNIT_*` 0–6. An unknown number becomes `none` and is logged in verbose mode (edge case:
unknown unit).

## Constraint

| Variant | JSON | Rules |
|---------|------|-------|
| None | `{"kind": "none"}` | |
| Range | `{"kind": "range", "min": n, "max": n, "step": n}` | `max` is inclusive; `step` 0 means any value. Fixed options are converted to decimals. |
| NumberList | `{"kind": "list", "values": [n, ...]}` | From a SANE word list (leading count word removed). |
| StringList | `{"kind": "strings", "values": ["...", ...]}` | Read up to the terminating null pointer. |

A null constraint pointer with a non-`None` type is treated as `None` (edge case: empty
choices).

## Value

| Variant | JSON | When |
|---------|------|------|
| Bool | `[true]` | `bool` options; each word ≠ 0 is true |
| Int | `[300]` | `int` options; arrays keep every element |
| Fixed | `[36.5]` | `fixed` options, converted to decimals |
| Text | `"Color"` | `string` options |

Numeric values are always JSON arrays so multi-value options are never truncated; the human
renderer prints single-element arrays as plain numbers.

## CapabilitySummary

Key film-scanning facts (FR-004), derived from the capability list. Missing data is `null` and
shown as "unknown".

| Field | Type | Source |
|-------|------|--------|
| `resolutions_dpi` | Constraint or null | `resolution` option's constraint |
| `color_modes` | list of strings or null | `mode` string list |
| `bit_depths` | list of integers or null | `depth` list |
| `scan_area_mm` | `{width, height}` or null | `br-x` and `br-y` range maximums |
| `infrared` | `"available"` / `"not available"` / `"unknown"` | see research D8 |

## CapabilityReport

What `capabilities` prints (spec entity "Capability Report"): `device` (DeviceInfo), `summary`
(CapabilitySummary), `options` (list of Capability in SANE order; groups appear in place so
the human output can use them as section headers).

## ScanError (spec entity "Detection Outcome")

Every non-success outcome. Each variant has a message, a next step and an exit code; the full
table is in [contracts/cli.md](contracts/cli.md).

| Variant | Carries |
|---------|---------|
| `NotFound` | nothing |
| `Busy` | device name, if known |
| `PermissionDenied` | device node path |
| `DriverUnavailable` | reason (driver file missing / scanner present but not recognized) |
| `Disconnected` | what was in progress |
| `Interrupted` | nothing |
| `Sane` | SANE status code and its `sane_strstatus` text (any other failure) |

## State and lifecycle

```text
start → write SANE config → install Ctrl-C handler → probe /sys
      → Sane::init ──fail──→ ScanError
      → list devices ──none──→ diagnose from /sys → ScanError
      → [detect] print devices → drop Sane → exit 0
      → [capabilities] open Device<'a> → read options (check Ctrl-C between each)
            → close Device (Drop) → exit SANE (Drop) → print report → exit 0
any error after init → values dropped in reverse order (device closed, SANE exited) → exit N
```
