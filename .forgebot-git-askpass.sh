#!/bin/sh
prompt="$1"
case "$prompt" in
  *Username*|*username*)
    printf '%s\n' "${FORGEBOT_FORGEJO_BOT_USERNAME:-forgebot}"
    ;;
  *)
    printf '%s\n' "${FORGEBOT_FORGEJO_TOKEN:-}"
    ;;
esac
