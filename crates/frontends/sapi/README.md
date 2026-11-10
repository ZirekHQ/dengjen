# dengjen-sapi

A SAPI5 voice for Windows that speaks with dengjen voices, including Kurmanji. SAPI5 loads it as an
in-process COM server, so Narrator (through "Add legacy voices"), NVDA's SAPI5 driver, JAWS and
any other SAPI5 program can use it, on the logon screen and in UAC prompts too when registered
machine-wide.

**Status: spike.** It speaks, aborts within one 40 ms slice of audio, and reports sentence, word
and bookmark events, but it has no installer yet.

## Layout on disk

Put these side by side, using the 32-bit build for 32-bit programs and the 64-bit build for
64-bit ones:

```
dengjen_sapi.dll
onnxruntime.dll          # same bitness, ONNX Runtime 1.27 or later
espeak-ng-data/
voices/piper/<key>/<key>.onnx
voices/piper/<key>/<key>.onnx.json
```

## Registering

- `regsvr32 dengjen_sapi.dll` from an elevated prompt registers every voice under `voices` for all
  accounts. `regsvr32 /u dengjen_sapi.dll` removes them.
- `regsvr32 /n /i:user dengjen_sapi.dll` registers for the current user only, without admin
  rights; `regsvr32 /u /n /i:user dengjen_sapi.dll` removes that.
- Options go in `/i:"..."`, separated by `;`: `user` or `machine`, `voices=<dir>` for another
  voices directory, and `kurdish=<policy>`.

Use the matching `regsvr32`: `C:\Windows\SysWOW64\regsvr32.exe` for the 32-bit DLL.

### The Kurdish language ID

Windows has no LANGID for Kurmanji. SAPI clients look voices up by LANGID, so `kurdish=` chooses
what Kurmanji voices declare:

| Policy | `Language` attribute | Effect |
|---|---|---|
| `unspecified` (default) | `1000` | Accurate; clients that only list known languages may hide the voice. |
| `both` | `1000;492` | Clients asking for Kurdish find it; the first LANGID stays unspecified. |
| `ckb` | `492` | Declares Central Kurdish (Sorani), for clients that drop unknown languages. |

## Design

- `fragments`, `speak`, `timing`, `prosody`, `language` and `registration` are platform-neutral,
  forbid `unsafe`, and are tested on every OS with fake SAPI and synthesizer objects.
- `windows` is the COM layer. Every entry point runs inside `catch_unwind`, so a panic becomes
  `E_FAIL` instead of unwinding into the host program.
- Text is synthesized one sentence at a time, and audio is written in 40 ms slices with an abort
  check before each, so a screen reader's purge stops speech quickly.
- Piper voices report no alignments, so word events are placed in proportion to each word's
  character position in its sentence; sentence events are exact.

## Building

```
cargo build --release -p dengjen-sapi --features ort-dylib --target x86_64-pc-windows-msvc
cargo build --release -p dengjen-sapi --features ort-dylib --target i686-pc-windows-msvc
```

`.github/workflows/sapi.yaml` builds both, stages them with ONNX Runtime, the eSpeak-ng data and the
Kurmanji voice, and runs `tests/smoke.ps1`, which speaks through SAPI's `SpVoice`.

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
