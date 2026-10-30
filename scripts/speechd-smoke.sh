#!/usr/bin/env bash
# Speaks through an installed dengjen-speechd package via a real Speech Dispatcher server.
# Runs inside a clean container that already has the .debs in /pkg and ca-certificates installed.
#   speechd-smoke.sh
set -euo pipefail

voice=ku_TR-berfin_renas-medium

apt-get install -y speech-dispatcher >/dev/null
useradd --create-home tester

su tester -s /bin/bash -c '
  set -euo pipefail
  voice="$1"
  dengjen voices install "$voice" >/dev/null
  mkdir -p ~/.config/speech-dispatcher
  # The server has no null audio plugin; ALSA with its null device discards the audio.
  printf "DefaultModule dengjen\nAudioOutputMethod \"alsa\"\nAudioALSADevice \"null\"\n" \
    > ~/.config/speech-dispatcher/speechd.conf

  spd-say -O | grep -qx dengjen
  spd-say -o dengjen -L | grep -q "$voice"
  # -w returns only after the module reports END, so a crashed module times out.
  timeout 60 spd-say -o dengjen -l ku -w "Silav, tu çawa yî?"
  timeout 60 spd-say -o dengjen -c a -w
  timeout 60 spd-say -o dengjen -k enter -w
  # A synthesis error mid-stream still ends with END, so read the module log for it.
  if grep "sd_dengjen:" ~/.cache/speech-dispatcher/log/dengjen.log; then
    exit 1
  fi
' _ "$voice"
