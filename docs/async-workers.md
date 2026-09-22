# Native background search

One native thread owns the populated HOME index, directory validation, and fuzzy
matching. The main thread owns the terminal and application state. Both use
absolute filesystem paths; there are no remounts, permissions or reload handshakes.

The worker advances the existing cooperative walker in bounded 128-entry slices
and checks demand between slices. It publishes progress at most every 100 ms,
plus completion immediately. Matching returns at most 100 paths / 64 KiB.

The UI permits one outstanding request; newer edits replace unsent demand.
The search scheduler observes the actual input text each UI iteration, including
while a request is outstanding. Any text change restarts the 120 ms quiet period;
there is no key/action list to maintain when adding editing commands. Cursor
movement and unchanged input do not restart it. Explicit validation bypasses the
delay, and F5's new refresh epoch permits immediate search of the restored form.
Epochs fence refreshes; generations and catalogue revisions reject
obsolete replies. Reset, help, cancellation and suggestion selection preserve the existing
core behavior. Request and response channels are bounded.

The exact invoking cwd can be validated directly even outside HOME or through a
symlink, without expanding search. A logical `$PWD` is retained only when it
resolves to the actual process cwd. UTF-8 decoding is strict; a legitimate U+FFFD
name is no longer rejected as if it were a lossy WASI path.

A missing response after 15 seconds disables search validation with a visible
error. There is no synchronous fallback or launch retry. A running match or slow
filesystem syscall cannot be forcibly cancelled; late replies remain fenced.
Quit does not wait for a stalled filesystem worker.

Core tests cover stale queries, validation, refresh and UI behavior. Native worker
tests use disposable filesystem roots; the live harness exercises real terminal
input and search. Historical WASM timing measurements are not native performance
claims.
