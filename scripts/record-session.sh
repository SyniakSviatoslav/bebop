#!/usr/bin/env bash
# Records a REAL bebop CLI session to an asciinema cast via asciinema.
# No faking: every command is a genuine bebop invocation on the real repo.
set -u
export NO_ANIM=1
export BEBOHeader=""
cd /root/bebop-repo

send() { # type a command then enter, with human-ish delay
  printf '%s' "$1"
  sleep 0.5
  printf '\r'
  sleep "$2"
}

# asciinema records the real PTY session
asciinema rec -t "Bebop — live session" -c "bash /root/bebop-repo/scripts/_rec_session_inner.sh" /root/bebop-repo/docs/footage/bebop-session.cast --overwrite
