# Security Policy

## Reporting a vulnerability

Report suspected vulnerabilities privately via
[GitHub Private Vulnerability Reporting](https://github.com/ZirekHQ/dengjen-tts/security/advisories/new)
(Security tab → Report a vulnerability). Do not open a public issue for a suspected
vulnerability.

Include, where possible: the affected crate/version, a minimal reproduction, and the impact
(memory safety, DoS, information disclosure, etc.).

## Scope

dengjen embeds several C libraries via FFI (onnxruntime, espeak-ng). The public C ABI
(`crates/frontends/capi`) is in scope, as is any `unsafe` code in
`crates/text/espeak-phonemizer`, and `dengjen-tts`'s FFI boundary. This also covers every other
crate that talks to `onnxruntime` via `ort` — `crates/frontends/cli`, `crates/frontends/grpc`,
`crates/frontends/python`, `crates/dengjen/models/{piper,kokoro,melotts}`, and
`crates/text/{pinyin,hebrew}-phonemizer`. Denial-of-service reports
against malformed voice model/config files are in scope; resource-exhaustion reports against
large-but-well-formed inputs are lower priority.

## Supported versions

This project does not yet maintain parallel release branches — security fixes land on `main`.
