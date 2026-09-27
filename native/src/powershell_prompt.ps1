# Runs after the profile so prompt themes remain in control of the visible prompt.
# Reuse the guard from the documented profile workaround to avoid double wrapping.
if ($null -eq $global:LaunchpadOriginalPrompt) {
    $global:LaunchpadOriginalPrompt = $function:prompt
    function global:prompt {
        # Call the original first so it sees the last command's status.
        $out = @(& $global:LaunchpadOriginalPrompt) -join ''
        $loc = $ExecutionContext.SessionState.Path.CurrentLocation
        if ($loc.Provider.Name -eq 'FileSystem') {
            try { [Environment]::CurrentDirectory = $loc.ProviderPath } catch {}
            $out += ([string][char]27) + ']9;9;' + $loc.ProviderPath + ([string][char]27) + '\'
        }
        $out
    }
}
