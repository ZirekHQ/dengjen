# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

## What is signed

Only the Windows SAPI5 voice is signed: `dengjen_sapi.dll` and its installer,
`dengjen-sapi-<version>-x64-setup.exe`, attached to each [release](https://github.com/ZirekHQ/dengjen/releases).
Both are built from this repository's source by GitHub Actions on GitHub-hosted runners
([`publish-sapi.yaml`](.github/workflows/publish-sapi.yaml), called by
[`release.yaml`](.github/workflows/release.yaml)), and nothing built elsewhere is submitted for
signing.

The installer also carries files that this project does not build and does not sign:

- `onnxruntime.dll`, from Microsoft's [ONNX Runtime](https://github.com/microsoft/onnxruntime)
  NuGet package (MIT);
- `vc_redist.x64.exe`, Microsoft's Visual C++ runtime installer, signed by Microsoft;
- the eSpeak-ng data, built from [eSpeak-ng](https://github.com/espeak-ng/espeak-ng)'s source
  (GPL-3.0), and the Kurmanji Piper voice model from
  [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices). These are data files, not
  programs.

Other release files (Linux packages, Android and Apple libraries, Python wheels) are not signed
under this policy.

## Team roles

- Committers and reviewers: [members of the ZirekHQ organization](https://github.com/orgs/ZirekHQ/people).
  Changes from anyone else are reviewed by a committer before they are merged.
- Approvers: [Ali Ustek](https://github.com/austek), owner of the ZirekHQ organization. Every
  signing request is approved by hand in SignPath before anything is signed.

All team members use multi-factor authentication for GitHub and SignPath.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it.

The SAPI5 voice synthesizes speech on the computer it is installed on. It makes no network
connections, and the installer downloads nothing: every file it installs is inside it.
