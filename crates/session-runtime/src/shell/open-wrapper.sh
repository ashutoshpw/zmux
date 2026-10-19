#!/bin/sh
exec "$ZMUX_SHELL_INTEGRATION_DIRECTORY/bin/zmux-open-url" --delegate "${0##*/}" "$@"
