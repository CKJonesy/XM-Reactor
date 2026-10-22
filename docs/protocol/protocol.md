# Sony headphone protocol

Shared reference for the XM Reactor protocol implementation.
Byte strings live in `vectors.json`; the test suite loads it. Add a vector there before changing behaviour.

## Frame format

```
3E  <escaped body>  3C
body = type(1) seq(1) length(4, big-endian) payload(length) checksum(1)
checksum = sum of all preceding body bytes, modulo 256
```

- Escaping applies to the body only. Bytes `3C`, `3D`, `3E` become `3D` followed by the byte minus `0x10` (`2C`, `2D`, `2E`).
- A frame is rejected if the markers, checksum or declared length are wrong.
- Message types: `01` Ack, `0C` DataMdr (table 1), `0E` DataMdrNo2 (table 2).

## ACK rule

Every DataMdr and DataMdrNo2 frame received is answered with an Ack frame (type `01`, empty payload) whose
seq is `1 - received seq`, wrapping as a byte. An Ack from the device carries the next transmit sequence;
the sender only accepts the Ack whose seq equals the one it is waiting for.

## Table 1 and table 2

Table 1 payloads travel in DataMdr frames: first byte is the command id, second is usually a sub-type.
Table 2 payloads travel in DataMdrNo2 frames and use a separate command space (wearing status only so far).
A reply is a RET (command id one above the get) or an NTFY (unsolicited, same layout as the RET).

## Version detection

1. After connecting, the only command sent is `00 00`. Nothing else is sent until the version is known, because the v2 battery query `22 00` is the v1 POWER OFF opcode.
2. The reply payload starts with `01`. Payload length 4 means protocol v1 (XM4-class). Payload length 8 means protocol v2 (XM5/XM6-class). Any other length is unrecognised and the app disconnects rather than guessing.
3. On v2 the reply (`v2.protocolInfo`) carries a version and two flags, `supportsTable1` and `supportsTable2` (wire value 0 = enabled).
4. On v2, send `v2.getSupportFunctions` (`06 00`); the announced function ids select the NC/ambient variant (`6B` 0x17, `6D` 0x19, `67` 0x22) and the battery kind.

## Verification status

`reference`: transcribed from a reference client. `live:<model>`: captured from a real headset.

## XM6 (live 2026-10-19)

Captured from a WH-1000XM6 (init reply `01 00 03 00 30 32 00 00`, firmware 0x03003032) with the probe. Vectors are labelled `live:WH-1000XM6`.

Capabilities: `model=WH1000XM6 version=V2`, 45 announced functions: 10 12 13 14 15 16 17 19 20 23 25 27 34 41 42 44 56 57 69 6D 71 90 94 A1 B0 B2 B6 B7 B8 C1 C2 D1 D2 D3 E2 E5 E6 E7 EB F1 F4 F9 FC FD FF. NC is `6D` (9-byte reply `67 19 ...`). Functions `50` and `52` are not announced; `56` and `57` are.

- **EQ lives under inquired type `04`.** Use `52 04`, `56 04` and `58 04 ...`. `56 00` still answers, but `58 00 ...` writes are ACKed and ignored. Replies and the headset's own notifications use `57 04` and `59 04`. The code selects type `04` when `57` is announced and `50` is not.
- **Accepted preset ids:** `00` off, `20` (GamingEq), `30` (Heavy), `31` (Clear), `32` (Hard), `33` (Soft), `A0` manual, `A1` custom 1, `A2` custom 2. The ids `01`, `10`, `11`, `21`, `22`, `23`, `34`, `35`, `36` and `40` are rejected by XM6 firmware 0x03003032 (protocol version from the init reply): the write is ACKed and the state is unchanged.
- **Auto power off accepts only taken-off and never:** `28 05 10 00` and `28 05 11 00` take effect, echoed as `29 05 ...`. Timer writes (`28 05 01 00`, `28 05 01 01`, `28 05 04 04`) are rejected by XM6 firmware 0x03003032: ACKed, state unchanged, notify `29 05 10 00`.
- 10-band EQ replies carry flat as `06` per band (wire offset +6); `0A` in byte 3 is the band count.

## XM6 additions (live 2026-10-20, firmware 3.1.5)

Results and set-and-restore details are in `probe-results.md`. Layouts below are `live:WH-1000XM6`.

| Function | Get bytes | Set bytes | Reply layout |
|---|---|---|---|
| Firmware | `04 02` | none | `05 02 len text` |
| Colour | `04 03` | none | `05 03 series colour` (series `30` premium; colour `01` black, `02` white, `03` silver, `04` red, `05` blue, `06` pink, `07` yellow, `08` green, `09` grey, `0A` gold) |
| Codec | `12 02` | none | `13 02 codec` (`10` LDAC) |
| BGM | `E6 09` | `E8 09 onoff room` (0 = on) | `E7/E9 09 onoff room` |
| Cinema | `E6 04` | `E8 04 onoff` (0 = on) | `E7/E9 04 onoff` |
| Device list (table 2) | `36 02` | none | `37/39 02 count (mac state 3 bytes len name)* playing` |
| NC/ambient button | `66 30` | read only for v1 | `67/69 30 function` (`02` NC and ambient) |
| Voice guidance volume (table 2) | `46 20` | read only for v1 | `47 20 volume` (signed, -2..+2) |
| General setting label | `D0 Dn 01` (n = slot 1..4, announced as functions `D1`..`D4`) | none | `D1 Dn type format len label len summary` |
| General setting value | `D6 Dn` | `D8 Dn 00 onoff` (0 = on); sidetone only in v1 | `D7/D9 Dn 00 onoff` |
| Sound effect status | none | none | `E5 01 value` (1 while BGM or Cinema is on; meaning unverified) |

- Adaptive volume `F6 0A` is ACKed with no reply: not supported.
- Auto ambient is byte [7] of the 9-byte `67 19` reply (the first adaptive byte).
- NC notifications carry byte [2] `00` (value changing) or `01`. Taking the headset off sends `F5 10 01 01` then an effect-off `69 19`; putting it on sends `F5 10 01 00` then effect on. An effect-off notification while removed is a suspension, not the user choosing Off.

## v2 vectors

| Function | Get bytes | Set bytes | Reply layout | Verified |
|---|---|---|---|---|
| Protocol info | `00 00` | none | `01 00 ver(4,BE) t1 t2` (0 = enabled) | reference |
| Support functions | `06 00` | none | `07 00 count (fn priority)*` | reference |
| NC/ambient, type 0x17 | `66 17` | `68 17 01 effect asmMode voice level` | `67/69 17 01 effect asmMode voice level` | reference |
| NC/ambient, type 0x19 | `66 19` | `68 19 01 effect asmMode voice level adaptiveOn sens` (zeros when not set) | as 0x17 plus `adaptiveOn sens`; a 7-byte reply has no adaptive data | reference |
| NC/ambient, type 0x22 | `66 22` | `68 22 01 effect voice level` | `67/69 22 01 effect voice level` | reference |
| EQ available | `52 00` | none | `53/55 00 value` (0 = available) | reference |
| EQ preset and bands | `56 00` | preset `58 00 preset 00`; 6-band `58 00 preset 06 cb b1..b5` (+10); 10-band `58 00 preset 0A b1..b10` (+6) | `57/59 00 preset count bands` | reference |
| Battery single | `22 00` (`22 08` with threshold) | none | `23/25 00 level charging` | reference |
| Battery left/right | `22 01` (`22 09`) | none | `23/25 01 left lc right rc` | reference |
| Battery cradle | `22 02` (`22 0A`) | none | `23/25 02 level charging` | reference |
| Power off | none | `24 03 01` | none | reference |
| Speak-to-Chat | `F6 0C` | `F8 0C onoff 01` (onoff 0 = on) | `F7/F9 0C onoff ..` (0 = on) | reference |
| Pause on removal | `F6 01` | `F8 01 onoff` (0 = on) | `F7/F9 01 onoff` (0 = on) | reference |
| DSEE | `E6 01` | `E8 01 value` (1 = on) | `E7/E9 01 value` (1 = on) | reference |
| Auto power off | `26 04` timer, `26 05` with wearing detection | `28 04/05 value 00` | `27/29 04/05 value ..` | reference |
| Wearing status (table 2) | `F2 00` | none | `F3/F5 00 code` (00 worn; 02, 03, 04 removed; else unknown) | reference |

Auto power off values: `00` 5 min, `04` 15 min, `01` 30 min, `02` 60 min, `03` 180 min, `10` when taken off, `11` never.

Setters are built only for announced functions: EQ needs `50`, `52` or `57`, power off needs `23`, and auto power off only sends a setting in the device's choice list (XM6: taken off and never). The 6-band or 10-band EQ writer is refused once an EQ reply has shown the other band count. A 0x19 write before any 9-byte 0x19 reply first sends `66 19` and waits for the reply (3 s), so the adaptive bytes are echoed rather than zeroed.

Notes: `effect` 0 = off, 1 = on. Ambient level is 0 to 20; replies above 20 are clamped to 20.
An unknown EQ preset byte is kept raw and rendered as `unknown(0xNN)`. Charging: 0 not charging, 1 charging, 2 unknown, 3 charged.

## v1 vectors

| Function | Get bytes | Set bytes | Reply layout | Verified |
|---|---|---|---|---|
| Protocol info | `00 00` | none | `01 00 ver(2,BE)` (4 bytes in total) | reference |
| NC/ambient | `66 02` | `68 02 effect type mode 01 voice level` (`type` echoes the last reply) | `67/69 02 effect type mode 01 voice level` (8 bytes) | reference |
| EQ | `56 01` | preset `58 01 preset 00`; bands `58 01 FF 06 cb b1..b5` (+10) | `57/59 01 preset 06 cb b1..b5` (+10) | reference |
| Battery | `10 kind` (0 single, 1 left/right, 2 cradle) | none | `11/13 00 level charging`; `01 l lc r rc`; `02 level charging` | reference |
| Power off | none | `22 00 01` | none | reference |
| Speak-to-Chat | `F6 05` | `F8 05 01 onoff` | `F7/F9 05 01 onoff` | reference |
| DSEE | `E6 02` | `E8 02 00 onoff` | `E7/E9 02 00 onoff` | reference |
| Pause on removal | `F6 03` | `F8 03 00 onoff` | `F7/F9 03 00 onoff` | reference |
| Auto power off | `F6 04` | `F8 04 01 c0 c1` | `F7/F9 04 01 c0 c1` | reference |

Status `reference` becomes `live:WH-1000XM4` once checked on hardware.

NC/ambient: set `effect` is `00` for off and `11` otherwise. `level` is the ambient level for off and ambient, and `00` for NC.
Effect "on" is `11` on the wire for SET but `01` in replies; replies use `00` for off.

Byte 3 (`type`) selects one of two accepted reply layouts. Any other combination parses to nothing.

- `type 02` (alternate layout): `mode` `02` NC, `01` wind-noise reduction, `00` ambient. Wind is read as noise cancelling and never written. Vectors `v1.ncAmb.nc`, `v1.ncAmb.ambient`.
- `type 01` (replayed on a real XM4, firmware 3.0.1, spec section 6.1): `mode` `02` NC, `00` ambient. Vectors `v1.ncAmb.nc.settingType01`, `v1.ncAmb.ambient.settingType01`.
- `type 00` (models without wind support): `mode` `01` NC, `00` ambient.

Echo rule: writes mirror the headset's own reply. The profile remembers `type` from the last non-off NC reply (default `02`) and writes it back at byte 3. With `type 01`, byte 4 is `02` for NC and `00` otherwise, with no wind value (`v1.setNcAmb.nc.settingType01` = `68 02 11 01 02 01 00 00`). With `type 02` or `00` the writer keeps the model-table layout: `02`/`02` on wind-capable models, `00`/`01` otherwise.

Setters only send what the model table allows; auto power off only sends a setting listed in the model's choices, and 10-band EQ writes are never built on v1.

Auto power off pairs `c0 c1`: `11 00` never, `00 00` 5 min, `01 01` 30 min, `02 02` 60 min, `03 03` 180 min, `10 00` when taken off. There is no 15 minute value on v1.
