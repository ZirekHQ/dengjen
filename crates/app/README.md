# dengjen-app

Logic shared by the Dengjen apps on Windows, Linux, Android and macOS, so each app only draws its controls and plays audio.

- `sentences` splits text into the sentences an app reads one at a time, with each sentence's range in UTF-8 bytes and in UTF-16 code units, so WinUI, Android, AppKit and GTK can all highlight the sentence being spoken. `sentence_at` finds the sentence under the caret, for "read from here".
- `import::text_from_bytes` turns a `.txt`, `.md` or `.html` file into plain text to read aloud. Text files may be UTF-8 or UTF-16 with a byte-order mark.

Audio export already exists in the engine (`libdengjenSpeakToFile` in the C API).

## Support

This crate is part of [Zirek](https://github.com/ZirekHQ). 💖 [Support on Open Collective](https://opencollective.com/zirek)
