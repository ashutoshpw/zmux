#!/bin/sh
# Session-only opener bridge. It emits only an explicit, single HTTP(S) URL.
# `/dev/tty` is the current terminal: nested tmux/screen may consume this OSC,
# so automatic opener routing from those nested panes is unsupported here.

zmux_error() {
    printf 'zmux: %s\n' "$1" >&2
    exit 2
}

zmux_is_http_url() {
    case "$1" in
        [hH][tT][tT][pP]://*|[hH][tT][tT][pP][sS]://*) return 0 ;;
        *) return 1 ;;
    esac
}

zmux_emit_url() {
    [ "$#" -eq 1 ] || zmux_error 'open integration accepts exactly one HTTP(S) URL'
    zmux_url=$1
    zmux_is_http_url "$zmux_url" || zmux_error 'open integration accepts only HTTP(S) URLs'
    case "$zmux_url" in
        *[[:space:][:cntrl:]]*|*\\*) zmux_error 'URL contains whitespace, controls or a backslash' ;;
    esac
    [ "${ZMUX_OPEN_URL_NONCE+x}" = x ] || zmux_error 'session opener is unavailable'
    case "$ZMUX_OPEN_URL_NONCE" in
        ''|*[!0123456789abcdef]*) zmux_error 'session opener is unavailable' ;;
    esac
    [ "${#ZMUX_OPEN_URL_NONCE}" -eq 32 ] || zmux_error 'session opener is unavailable'

    zmux_bytes=$(LC_ALL=C printf '%s' "$zmux_url" | wc -c | tr -d '[:space:]') ||
        zmux_error 'cannot measure URL length'
    case "$zmux_bytes" in
        ''|*[!0-9]*) zmux_error 'cannot measure URL length' ;;
    esac
    [ "$zmux_bytes" -le 8192 ] || zmux_error 'URL exceeds 8192 bytes'

    zmux_encoded=$(printf '%s' "$zmux_url" | base64 | tr '+/' '-_' | tr -d '=\r\n') ||
        zmux_error 'cannot encode URL'
    [ -n "$zmux_encoded" ] || zmux_error 'cannot encode URL'
    if ! printf '\033]777;zmux-open;v1;%s;%s\007' \
        "$ZMUX_OPEN_URL_NONCE" "$zmux_encoded" >/dev/tty; then
        zmux_error 'cannot send open request to this terminal'
    fi
}

zmux_delegate() {
    zmux_name=$1
    shift
    zmux_is_open_url=0
    if [ "$#" -eq 1 ] && zmux_is_http_url "$1"; then
        zmux_is_open_url=1
    fi
    if [ "$zmux_is_open_url" -eq 1 ]; then
        zmux_emit_url "$1"
        return $?
    fi

    zmux_root=${ZMUX_SHELL_INTEGRATION_DIRECTORY-}
    [ -n "$zmux_root" ] || zmux_error 'session opener is unavailable'
    zmux_path=${PATH-}:
    while [ -n "$zmux_path" ]; do
        zmux_directory=${zmux_path%%:*}
        zmux_path=${zmux_path#*:}
        [ -n "$zmux_directory" ] || zmux_directory=.
        [ "$zmux_directory" = "$zmux_root/bin" ] && continue
        zmux_candidate=$zmux_directory/$zmux_name
        if [ -x "$zmux_candidate" ] && [ ! -d "$zmux_candidate" ] &&
            [ "$zmux_candidate" != "$0" ]; then
            exec "$zmux_candidate" "$@"
        fi
    done
    printf 'zmux: no original %s executable is available in PATH\n' "$zmux_name" >&2
    return 127
}

if [ "${1-}" = --delegate ]; then
    [ "$#" -ge 2 ] || zmux_error 'invalid opener delegation'
    shift
    zmux_delegate "$@"
fi

zmux_emit_url "$@"
