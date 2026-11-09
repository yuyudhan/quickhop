# Using quickhop

Everything quickhop does is available from the `quickhop` command: search, run results, script
commands, clipboard history, snippets, file search and the background daemon. On macOS the
daemon also shows the launcher window, opened by the global `hotkey` (`Alt+Space` unless you
set another); on Linux the window is still being built (see [plan.md](plan.md)).

![quickhop in a terminal: a unit conversion, the lock-screen command, a GitHub quicklink, a script command, clipboard history and a snippet](media/quickhop-cli.gif)

## A 30-second tour

    quickhop query 5 km to miles               # calculator and units
    quickhop query lock screen --limit 1       # system commands, apps, emoji, files, ...
    quickhop run hello Ada                     # a Raycast script command
    echo 'ssh deploy@10.0.0.7' | quickhop clipboard add
    quickhop clipboard list                    # history, newest first
    quickhop launch snippets:reply --arg Ada   # a snippet with its placeholders filled

Each result line reads `title  (subtitle)  [id]`. The id (`provider:key`) is how you run a
result, configure it in `[[commands]]`, or script against quickhop.

## Search

    quickhop query firefox
    quickhop query '2^10 + 5%'                 # quote text the shell would mangle
    quickhop query -- -5 + 3                   # `--` before text that starts with a hyphen
    quickhop query term --limit 3 --json       # everything, machine-readable

Every enabled provider answers each search: applications, calculator and units, emoji, system
commands, quicklinks, script commands, snippets and files. Fuzzy matching tolerates gaps and
typos in order (`lckscr` finds Lock Screen), and ranking learns from what you pick: each
`launch` or `open` records the pick for that query in `frecency.sqlite`, so the result you use
most rises to the top.

Results come in tiers: **pinned** (an alias you typed), **matched** (fuzzy matches, ranked by
score and frecency) and **computed** (calculator answers).

## Run a result

    quickhop launch emoji:1f604 --query smile          # prints 😄 (its default action: copy)
    quickhop launch apps:org.mozilla.firefox.desktop   # opens Firefox
    quickhop launch system:lock --yes                  # power commands ask for --yes
    quickhop launch files:/home/me/notes.md --action reveal
    quickhop launch snippets:reply --arg Ada --paste   # paste into the focused app
    quickhop launch system:shutdown --dry-run          # print the action as JSON, run nothing

`--action` picks the result's first action of a kind instead of its default one: `open`,
`copy`, `run`, `script`, `snippet`, `reveal` or `trash`; an unknown kind lists the ones the
result has. `--query` passes the text the result was found with, which quicklinks and the
calculator need.

`quickhop open` takes the same options but runs the action inside the daemon, starting one if
none runs. Bind it to a key in your desktop or compositor (`quickhop open system:lock --yes`)
for a global shortcut to any result.

## Script commands

Drop an executable with Raycast `@raycast.*` comments into `~/.config/quickhop/scripts`:

    quickhop run hello Ada                     # by stem, file name, title or path
    quickhop run backup --yes                  # confirm a needsConfirmation script
    quickhop run slow-report --timeout 300     # seconds before it is killed (default 60)

The full format, every Raycast field and what is supported: [script-commands.md](script-commands.md).

## Clipboard history

    quickhop clipboard watch                   # record every copied text until Ctrl+C
    quickhop clipboard list                    # newest first; --limit 50, --json
    quickhop clipboard paste 2                 # paste the second newest into the focused app
    quickhop clipboard add < notes.txt         # record text from standard input
    quickhop clipboard clear                   # delete the whole history

The daemon watches the clipboard for you, so after `quickhop autostart on` you never run
`watch` yourself. Watching works on macOS (the pasteboard, checked every 250 ms), on X11, and
on Wayland compositors with the data-control protocol (sway, Hyprland, KDE Plasma, COSMIC)
through `wl-paste --watch`. Only text is kept, up to 1 MiB per entry; blank copies and copies
a password manager marks as concealed or transient are never recorded. Copying the same text
again moves it to the top and counts the copy instead of adding a duplicate. Retention follows
`[clipboard]` in the config.

`paste` puts the entry on the clipboard and presses the paste key for you: ⌘V on macOS once
quickhop has the Accessibility permission, Ctrl+V on X11. On Wayland, or on macOS without the
permission, it copies the entry and says why it did not paste, so you press the key yourself.
Platform details: [platform-support.md](platform-support.md).

## Snippets

Snippets live in `~/.config/quickhop/snippets.toml` and use Raycast's placeholders:

```toml
[[snippets]]
name = "Reply"
keyword = ";re"
text = "Hi {argument name=\"name\"}, thanks for the report. Fixed on {date format=\"EEEE, MMM d\"}."
```

    quickhop query ';re'                              # find it by keyword or name
    quickhop launch snippets:reply --arg Ada          # Hi Ada, thanks for the report. Fixed on Monday, Nov 9.
    quickhop launch snippets:reply --arg Ada --paste  # straight into the focused app

Every placeholder and modifier: [configuration.md](configuration.md#placeholders).

## Quicklinks and aliases

```toml
[[quicklinks]]
name = "GitHub search"
url = "https://github.com/search?q={query}"
alias = "gh"

[[commands]]
id = "system:lock"
alias = "lk"
```

`quickhop query gh tokio` pins the GitHub search for `tokio` as the first result, and
`quickhop query lk` pins Lock Screen. Any result can get an alias, a global hotkey, a fixed
position in the empty search, or be turned off: [configuration.md](configuration.md#commands).

## Files and folders

File search uses quickhop's own index once you have built it, on macOS and Linux alike. On
macOS without an index it asks Spotlight (`mdfind`) from the third character on, up to 200
paths within 2 seconds.

    quickhop index                             # build it once (default: your home folder)
    quickhop index --watch                     # keep it current; the daemon does this for you

Choose what is indexed with `[files] roots`, `hidden` and `exclude`. File results open with the
default app and offer reveal, copy path, Quick Look (macOS) and move to trash.

## The daemon

One background process per user keeps the clipboard history and the live file index,
answers `open` and `reload`, and on macOS owns the launcher window and its global hotkey:

    quickhop autostart on                      # start at login; `off` and `status` too
    quickhop daemon                            # run it in the foreground
    quickhop daemon status                     # what it runs and watches, the hotkey it holds
    quickhop toggle                            # show the window, or hide it (macOS)
    quickhop show                              # also `quickhop hide`
    quickhop reload                            # re-read the config without a restart
    quickhop daemon stop

`toggle`, `show` and `open` start a daemon when none runs. A second `quickhop daemon` exits at
once: a lock file in the runtime directory keeps it to one. `quickhop daemon --no-window` runs
it without the window and the hotkey.

In the window, type to search, `↑`/`↓` to move, `Return` to run the selected result, `⌘K` to
list all its actions (type to filter them) and `Esc` to close the panel, then the window. It
also hides when another app takes the focus.

Script commands run from the window as their mode says: `fullOutput` opens the output (with
its colours) and `Esc` goes back, `compact` shows the last line as a toast, `silent` hides the
window and shows the line briefly over the app you were in, and `inline` scripts with a
`refreshTime` keep their subtitle current while the daemon runs. A script that asks for
arguments opens its fields in the search bar: type its alias and a space (or press `Return`
on it), fill the fields, `Tab` between them, `↑`/`↓` through a dropdown's choices, then
`Return`.

## Configuration and diagnostics

    quickhop config path                       # the file in use
    quickhop config default                    # every setting with its default, as TOML
    quickhop config check [FILE]               # validate before you rely on it
    quickhop doctor                            # platform, paths, session, tools, providers

`quickhop doctor` is the first thing to run when something looks wrong; see
[troubleshooting.md](troubleshooting.md).

## For scripts and other programs

- `quickhop query --json` prints one JSON document: the query, each result's id, provider,
  title, subtitle, icon, tier, score and actions, and any provider errors and warnings.
- `quickhop launch ID --dry-run` prints the action a result would run, as JSON.
- `quickhop clipboard list --json` prints each entry's text, copy count and first and last
  copy times (Unix seconds).
- Exit status is 0 on success and 1 on any error, with the reason on standard error, prefixed
  `quickhop:`.
