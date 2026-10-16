# Original zmux session-only Claude completion integration (MIT).
function claude
    if test "$ZMUX_CLAUDE_INTEGRATION_DISABLED" = 1
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
