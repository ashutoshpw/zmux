# Original zmux session-only Claude completion integration (MIT).
function claude
    if test "$ZMUX_CLAUDE_INTEGRATION_DISABLED" = 1; or not test -r "$ZMUX_SHELL_INTEGRATION_DIRECTORY/plugin/hooks/hooks.json"; or not test -p "$ZMUX_SHELL_INTEGRATION_DIRECTORY/attention"
        command claude $argv
        return $status
    end
    if test (count $argv) -gt 0
        switch $argv[1]
            case --help -h --version -v auth doctor install update upgrade mcp plugin plugins
                command claude $argv
                return $status
        end
    end
    command claude --plugin-dir "$ZMUX_SHELL_INTEGRATION_DIRECTORY/plugin" $argv
end

# fish config runs after --init-command, so refresh before each prompt as well.
function __zmux_refresh_openers
    set -q ZMUX_OPEN_URL_NONCE; or return 0
    set -l zmux_open_bin "$ZMUX_SHELL_INTEGRATION_DIRECTORY/bin"
    test -x "$zmux_open_bin/open"; or return 0
    set -l zmux_new_path "$zmux_open_bin"
    for zmux_path_entry in $PATH
        if test "$zmux_path_entry" != "$zmux_open_bin"
            set --append zmux_new_path "$zmux_path_entry"
        end
    end
    set --global --export PATH $zmux_new_path
end
function __zmux_refresh_openers_before_prompt --on-event fish_prompt
    __zmux_refresh_openers
end
__zmux_refresh_openers
