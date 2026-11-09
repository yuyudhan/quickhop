# quickhop documentation

## Using quickhop

| Page | Read it to |
|------|-----------|
| [install.md](install.md) | Install from source, as a `.deb`, or with the installer script; update and uninstall |
| [usage.md](usage.md) | Take the 30-second tour and learn every command: search, results, scripts, clipboard, snippets, files, the daemon |
| [configuration.md](configuration.md) | Look up every `config.toml` and `snippets.toml` key with its default, and every snippet placeholder |
| [script-commands.md](script-commands.md) | Write script commands, and see which Raycast metadata fields are supported |
| [platform-support.md](platform-support.md) | See what works on macOS, X11 and Wayland, and which permissions are needed |
| [troubleshooting.md](troubleshooting.md) | Go from an error message to its fix |

## Where quickhop is going

| Page | Read it to |
|------|-----------|
| [extensions.md](extensions.md) | Understand the three ways to extend quickhop and the design of its own extension API |
| [ai.md](ai.md) | See the decided AI design: local models through Ollama by default, your own keys for hosted ones |
| [features.md](features.md) | Browse all 95 Raycast features, their status in quickhop, and how each works on macOS, X11 and Wayland |
| [plan.md](plan.md) | Follow the build plan: slices, target machines and real UI acceptance tests |

## Working on quickhop

| Page | Read it to |
|------|-----------|
| [architecture.md](architecture.md) | Learn the crates, directories, process model, providers, index, storage and packaging |
| [testing.md](testing.md) | Run and extend the tests: provider contract, fixtures, snapshots, gates |
| [adr/0001-ui-toolkit.md](adr/0001-ui-toolkit.md) | See why the launcher window uses gpui, with measurements |
| [adr/0002-licence-extensions-ai.md](adr/0002-licence-extensions-ai.md) | See the licence, extension and AI decisions |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | Set up, run `just check`, follow the rules |

The terminal recording in [media/](media/) was captured from the real `quickhop` release build
under Xvfb on Ubuntu 22.04.
