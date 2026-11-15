# dengjen-sapi

A SAPI5 voice for Windows that speaks with dengjen voices, including Kurmanji. SAPI5 loads it as an
in-process COM server, so Narrator (through "Add legacy voices"), NVDA's SAPI5 driver, JAWS and
any other SAPI5 program can use it, on the logon screen and in UAC prompts too when registered
machine-wide.

**Status: spike.** It speaks, aborts within one 40 ms slice of audio, and reports sentence, word
and bookmark events. dengjen releases from v3.2.0 include a 64-bit installer.

## Installing

Run `dengjen-sapi-<version>-x64-setup.exe` from a `vX.Y.Z` [release](https://github.com/ZirekHQ/dengjen/releases),
v3.2.0 or later (the `grpc-release-*` and `java-*` releases do not carry it), or, between releases, the `dengjen-sapi-x64-setup` artifact of the `SAPI5 voice` workflow, as an
administrator. It installs the Visual C++ runtime if it is missing,
copies the voice to `C:\Program Files\Dengjen\SAPI5 voice` and registers it machine-wide, so
Narrator, NVDA's SAPI5 driver and other SAPI5 programs list it, on the lock screen and in UAC
prompts too. Uninstalling from Settings > Apps removes the files, the voice tokens and the COM
registration. For an unattended install, pass `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`. If the
Visual C++ runtime it installs needs a restart, Setup stops before copying anything (exit code 8
when unattended); restart Windows and run it again.

### Windows SmartScreen warning

The installer is not code signed yet, so Windows SmartScreen shows "Windows protected your PC"
when you run it. Select **More info**, check that the app is `dengjen-sapi-<version>-x64-setup.exe`,
then select **Run anyway**. Before you do, check that the file is the one the release published:
each release has a `SHA256SUMS-sapi.txt` beside the installer, and GitHub shows the same SHA-256
next to the asset. In PowerShell, in the folder you downloaded it to (use the file name of the
installer you downloaded):

```powershell
(Get-FileHash .\dengjen-sapi-3.2.0-x64-setup.exe -Algorithm SHA256).Hash
```

Compare the result with the line in `SHA256SUMS-sapi.txt` (case does not matter). For v3.2.0 it is
`dc817f726a035a172a41d628cd76f991ab95b99c6823a725b8b442626ffca710`. If it differs, do not run the
file; download it again from the release page. The `dengjen-sapi-x64-setup` workflow artifact has
no checksum file, and its hash does not match any release, so use it only for testing. Signing is tracked in
[#574](https://github.com/ZirekHQ/dengjen/issues/574).

The script is `installer/dengjen-sapi.iss` (Inno Setup 6); `tests/installer.ps1` installs, speaks
and uninstalls in CI. Releases build it with `publish-sapi.yaml`, which signs the DLL and the installer
through SignPath Foundation once the `SIGNPATH_ORGANIZATION_ID` variable and `SIGNPATH_API_TOKEN`
secret are set; until then it is unsigned and SmartScreen warns about it. See the
[code signing policy](../../../CODE_SIGNING_POLICY.md).

## Layout on disk

Put these side by side. Only the 64-bit build can run today: Microsoft publishes no 32-bit ONNX
Runtime after 1.22, so 32-bit programs cannot load the voice.

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

Use the 64-bit `regsvr32` in `C:\Windows\System32`.

### The Kurdish language ID

Windows has no LANGID for Kurmanji. SAPI clients look voices up by LANGID, so `kurdish=` chooses
what Kurmanji voices declare:

| Policy | `Language` attribute | Effect |
|---|---|---|
| `both` (default) | `492;1000` | NVDA reads only the first LANGID and reports `ckb`; clients asking for the unspecified `1000` still find the voice. Kurmanji is labelled as Central Kurdish. |
| `unspecified` | `1000` | Accurate, but NVDA reports the unknown locale `aa_ET` and logs symbol-table errors, and clients that only list known languages may hide the voice. |
| `ckb` | `492` | Declares Central Kurdish (Sorani) only, for clients that drop unknown languages. |

## Design

- `fragments`, `speak`, `timing`, `prosody`, `language` and `registration` are platform-neutral,
  forbid `unsafe`, and are tested on every OS with fake SAPI and synthesizer objects.
- `windows` is the COM layer. The COM methods and the registration exports run inside
  `catch_unwind`, so a panic becomes `E_FAIL` instead of unwinding into the host program;
  `DllMain`, `DllGetClassObject` and `DllCanUnloadNow` do only trivial work and are not guarded.
- Text is synthesized one sentence at a time on a worker thread while the `Speak` thread checks
  SAPI's abort and skip flags every 5 ms; either one interrupts the ONNX inference instead of
  waiting for the sentence to finish. Audio is written in 40 ms slices with the same check before
  each, so a screen reader's purge stops speech quickly and a skip takes effect mid-sentence.
- Screen readers speak their own UI in English (Narrator's `Misspelled`, `selected`, control
  types). For a Kurmanji voice, `english` respells a short list of such words in Kurmanji
  spelling before synthesis, so they sound English rather than being read with Kurmanji letter
  rules. Add words to its table as they turn up; the word must not also be a Kurmanji word.
- Piper voices report no alignments, so word events are placed in proportion to each word's
  character position in its sentence; sentence events are exact.

## Building

```
cargo build --release -p dengjen-sapi --features ort-dylib --target x86_64-pc-windows-msvc
cargo build --release -p dengjen-sapi --features ort-dylib --target i686-pc-windows-msvc
```

`.github/workflows/sapi.yaml` builds both and stages the 64-bit one with ONNX Runtime, the eSpeak-ng
data and the Kurmanji voice. It then runs `tests/smoke.ps1`, which speaks through SAPI's `SpVoice`.
The 32-bit DLL is only compiled.

## Diagnostics

A voice runs inside someone else's process, so there is no console. Set the environment variable
`DENGJEN_SAPI_LOG` to a file path (for Narrator, machine-wide with `setx /M`, then restart it) and
the DLL appends a line for each `Speak` request, each step with the first 40 characters of the text, the bytes written and each failed write. The log contains the text being read, so keep it off outside debugging. Without the variable, nothing is logged.

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
