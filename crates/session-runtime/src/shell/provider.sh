# Session-only Codex and OpenCode lifecycle adapters. User configuration stays
# in the provider; managed SSH adds only invocation-scoped files and hooks.
codex() {
    if [ "${ZMUX_REMOTE_LIFECYCLE-}" = 1 ] &&
        [ -r "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" ]; then
        if command -v python3 >/dev/null 2>&1; then
            command python3 "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" codex -- "$@"
            return $?
        fi
        printf 'zmux: remote Codex tracking requires Python 3; launching without lifecycle tracking\n' >&2
        command \codex "$@"
        return $?
    fi
    if [ "${ZMUX_LOCAL_CLAUDE_LIFECYCLE-}" = 1 ]; then
        if [ -x "${ZMUX_CLI_PATH-}" ] && [ -n "${ZMUX_SURFACE_ID-}" ]; then
            "$ZMUX_CLI_PATH" hooks provider auto codex \
                --root "$ZMUX_SHELL_INTEGRATION_DIRECTORY" \
                --surface "$ZMUX_SURFACE_ID" -- "$@"
            return $?
        fi
        printf 'zmux: automatic local Codex tracking is unavailable; launching once without lifecycle tracking\n' >&2
    fi
    command \codex "$@"
}

opencode() {
    if [ "${ZMUX_REMOTE_LIFECYCLE-}" = 1 ] &&
        [ -r "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" ]; then
        if command -v python3 >/dev/null 2>&1; then
            command python3 "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" opencode -- "$@"
            return $?
        fi
        printf 'zmux: remote OpenCode tracking requires Python 3; launching without lifecycle tracking\n' >&2
        command \opencode "$@"
        return $?
    fi
    if [ "${ZMUX_LOCAL_CLAUDE_LIFECYCLE-}" = 1 ]; then
        if [ -x "${ZMUX_CLI_PATH-}" ] && [ -n "${ZMUX_SURFACE_ID-}" ]; then
            "$ZMUX_CLI_PATH" hooks provider auto opencode \
                --root "$ZMUX_SHELL_INTEGRATION_DIRECTORY" \
                --surface "$ZMUX_SURFACE_ID" -- "$@"
            return $?
        fi
        printf 'zmux: automatic local OpenCode tracking is unavailable; launching once without lifecycle tracking\n' >&2
    fi
    command \opencode "$@"
}
