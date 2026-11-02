# Session-only Codex and OpenCode tracking. Provider settings stay user-owned.
function codex
    if test "$ZMUX_REMOTE_LIFECYCLE" = 1; and test -r "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py"
        if command -q python3
            command python3 "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" codex -- $argv
            return $status
        end
        printf 'zmux: managed SSH Codex tracking requires Python 3; launching once without lifecycle tracking\n' >&2
        command codex $argv
        return $status
    end
    if test "$ZMUX_LOCAL_CLAUDE_LIFECYCLE" = 1
        if test -x "$ZMUX_CLI_PATH"; and test -n "$ZMUX_SURFACE_ID"
            "$ZMUX_CLI_PATH" hooks provider auto codex --root "$ZMUX_SHELL_INTEGRATION_DIRECTORY" --surface "$ZMUX_SURFACE_ID" -- $argv
            return $status
        end
        printf 'zmux: automatic local Codex tracking is unavailable; launching once without lifecycle tracking\n' >&2
    end
    command codex $argv
end

function opencode
    if test "$ZMUX_REMOTE_LIFECYCLE" = 1; and test -r "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py"
        if command -q python3
            command python3 "$ZMUX_SHELL_INTEGRATION_DIRECTORY/provider-launch.py" opencode -- $argv
            return $status
        end
        printf 'zmux: managed SSH OpenCode tracking requires Python 3; launching once without lifecycle tracking\n' >&2
        command opencode $argv
        return $status
    end
    if test "$ZMUX_LOCAL_CLAUDE_LIFECYCLE" = 1
        if test -x "$ZMUX_CLI_PATH"; and test -n "$ZMUX_SURFACE_ID"
            "$ZMUX_CLI_PATH" hooks provider auto opencode --root "$ZMUX_SHELL_INTEGRATION_DIRECTORY" --surface "$ZMUX_SURFACE_ID" -- $argv
            return $status
        end
        printf 'zmux: automatic local OpenCode tracking is unavailable; launching once without lifecycle tracking\n' >&2
    end
    command opencode $argv
end
