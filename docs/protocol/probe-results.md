# XM6 probe results (version-1 additions)

Status: run on 2026-10-20 (about 16:30 AEDT) against the owner's WH-1000XM6, firmware 3.1.5 (protocol version 0x03003032), with the owner present and the vendor app closed.

Tool: `cargo run -p xm-core --bin xm-probe -- raw <hex> [--table2] [--wait <ms>]`. It prints `ACK ok` or `ACK timeout`, then every event and raw frame for the wait window (default 800 ms). Exit codes: 0 ok, 1 usage, 2 no headset Ready within 20 s, 3 command failed. `raw` refuses payloads starting `24` (power off family) unless `--confirm` is given. Every probe below used `--wait 1200`.

Run date: 2026-10-20  Firmware: 3.1.5 (`raw 04 02`)

| # | Probe | Table | Command line | Purpose | Reply or timeout | Set-and-restore result | Date | Firmware |
|---|---|---|---|---|---|---|---|---|
| 1 | Firmware version | 1 | `xm-probe raw 04 02` | Firmware string for this record | parsed Firmware("3.1.5") (length-prefixed form; raw bytes not recorded) | n/a (read only) | 2026-10-20 | 3.1.5 |
| 2 | Codec | 1 | `xm-probe raw 12 02` | Active codec reply layout | parsed Codec(Ldac) (raw bytes not recorded) | n/a (read only) | 2026-10-20 | 3.1.5 |
| 3 | BGM (background music) | 1 | `xm-probe raw E6 09` | Does the XM6 answer; reply layout | `E7 09 01 02` (off, room Cafe) | `E8 09 00 02` → ACK, `E9 09 00 02`, `E5 01 01`; restore `E8 09 01 02` → ACK; final `E7 09 01 02`. Works | 2026-10-20 | 3.1.5 |
| 4 | Cinema | 1 | `xm-probe raw E6 04` | Does the XM6 answer; reply layout | `E7 04 01` (off) | `E8 04 00` → ACK, `E9 04 00`, `E5 01 01`; restore `E8 04 01` → ACK, `E9 04 01`, `E5 01 00`. Works | 2026-10-20 | 3.1.5 |
| 5 | Device list | 2 | `xm-probe raw --table2 36 02` | Paired and connected device list layout | `37 02 ...`: parsed; the PC connected and playing, a phone not connected, and the rest of the paired list | n/a (read only) | 2026-10-20 | 3.1.5 |
| 6 | Adaptive volume | 1 | `xm-probe raw F6 0A` | Does the XM6 answer; reply layout | ACK, no reply: not supported | not attempted | 2026-10-20 | 3.1.5 |
| 7 | Auto ambient | 1 | `xm-probe raw 66 19` | Does the XM6 report it | byte [7] of the 9-byte `67 19` reply; 0 (off) at the time | not attempted (read only for v1) | 2026-10-20 | 3.1.5 |
| 8 | Touch sensor | 1 | `xm-probe raw D6 D3` | Does the XM6 answer | general setting slot 3, `D7 D3 00 00` (on) | `D8 D3 00 01` → ACK, `D9 D3 00 00` (stayed on); restore → ACK timeout; final `D7 D3 00 00`. Refused: read only for v1 | 2026-10-20 | 3.1.5 |
| 9 | NC/Ambient button | 1 | `xm-probe raw 66 30` | Does the XM6 answer | `67 30 02` (NC and ambient) | not attempted (set layout partly documented) | 2026-10-20 | 3.1.5 |
| 10 | Voice guidance volume | 2 | `xm-probe raw --table2 46 20` | Does the XM6 answer | `47 20 00` (signed volume 0, range −2..+2) | not attempted (set layout partly documented) | 2026-10-20 | 3.1.5 |
| 11 | Capture voice during calls | 1 | `xm-probe raw D6 D1` | Does the XM6 answer | general setting slot 1, `D7 D1 00 00` (on) | `D8 D1 00 01` → ACK, `D9 D1 00 01`; restore `D8 D1 00 00` → ACK, `D9 D1 00 00`. Works | 2026-10-20 | 3.1.5 |
| 12 | Colour / device info | 1 | `xm-probe raw 04 03` | Does the headset report its colour? | `05 03 30 03`: series 0x30 (premium), colour 03 (silver) | n/a | 2026-10-20 | 3.1.5 |

## General setting slots

The XM6 announces support functions `D1`, `D2` and `D3`, one per general setting slot. `D0 Dn` (capability, with an optional language byte `01`) returns `D1 Dn type format <len>label <len>summary`; `D6 Dn` returns `D7 Dn 00 vv` (`00` on, `01` off); `D8 Dn 00 vv` sets it and the headset echoes `D9 Dn 00 vv`.

| Slot | Capability reply | Value | Meaning |
|---|---|---|---|
| 1 | `D1 D1 00 01 10 "SIDETONE_SETTING" 18 "SIDETONE_SETTING_SUMMARY"` | `D7 D1 00 00` (on) | Capture voice during a phone call |
| 2 | label `MULTIPOINT_SETTING`, summary `..._LDAC_AVAILABLE` | `D7 D2 00 00` (on) | Multipoint on/off |
| 3 | label `TOUCH_PANEL_SETTING` | `D7 D3 00 00` (on) | Touch sensor |
| 4 | no reply to `D0 D4` or `D6 D4` | none | Absent |

Writes are allowed only for the slot labelled `SIDETONE_SETTING`. The touch sensor refused a write (ACKed, stayed on, and the restore timed out), so it is read only. Multipoint is read only in v1: the toggle was not tried, because turning it off could drop the PC link.

## Sound effect status

`E5 01 01` arrives when BGM or Cinema switches on, and `E5 01 00` when it switches off. The likely meaning is "sound effects (DSEE/EQ) are unavailable in this listening mode" (unverified). The code exposes it as a flag; EQ stays usable because no EQ write was refused.

## Live interactions (about 16:38 AEDT)

- Taps and swipes (play/pause, volume) send no MDR frames; Windows media keys handle them.
- NC/Ambient button: `69 19 01 01 01 01 14 00 00` (ambient).
- Taking the headset off: `F5 10 01 01`, then `69 19 00 00 01 01 14 00 00` (byte [2] `00` = value changing, byte [3] `00` = effect off while removed).
- Putting it on: `F5 10 01 00`, then `69 19 00 01 01 01 14 00 00` (effect back on).
- Many `69 19` notifications carry byte [2] = `00`; the parser accepts `00` and `01`. While the headset is off the head, an effect-off notification is the headset suspending noise cancelling, not the user choosing Off.

## Set-and-restore

For every row that answers, send one set built from the reply (flip a single value), read it back, then send the original value and read again. Record the set bytes, the echo, and whether the restore matched the original. Use `--wait 1500` so the notify frame is captured. Do not send anything starting `24`.
