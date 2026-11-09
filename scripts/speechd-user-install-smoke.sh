#!/usr/bin/env bash
# Installs the release .debs for one user with install-speechd-user.sh and checks that a real
# Speech Dispatcher server loads the module before any voice is installed.
# Runs inside a clean container that has the .debs in /pkg.
#   speechd-user-install-smoke.sh
set -euo pipefail

apt-get install -y speech-dispatcher >/dev/null
useradd --create-home tester

su tester -s /bin/bash -c '
  set -euo pipefail
  bash /scripts/install-speechd-user.sh --deb-dir /pkg --no-voice
  ~/.local/bin/dengjen voices --help >/dev/null
  # The server has no null audio plugin; ALSA with its null device discards the audio.
  printf "AudioOutputMethod \"alsa\"\nAudioALSADevice \"null\"\n" > ~/.config/speech-dispatcher/speechd.conf

  spd-say -O | grep -qx dengjen
  # With no voice yet, a request still ends instead of leaving the client waiting.
  timeout 60 spd-say -o dengjen -w "Silav"
  grep -q "dengjen voices install" ~/.cache/speech-dispatcher/log/dengjen.log
'
