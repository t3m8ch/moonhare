# Keep the Reviewer's Preferences in one `key = value` file under `$XDG_STATE_HOME`

The theme `:theme` chose and the Layout `s` or `:layout` chose hold in every repository and every later run. They are kept in `$XDG_STATE_HOME/moonhare/preferences` (`~/.local/state/moonhare/` without the variable; `~/Library/Application Support/moonhare/` on macOS unless it is set), not beside the Review in `.git/moonhare/`: they belong to the Reviewer, not to a clone. Without a choice the theme is dark, not `auto`: a dark terminal is the common case, and a light frame flashing in before the reply to OSC 11 (or a wrong guess when the terminal never replies) was worse than asking the few with a light terminal to say so once.

## Format

One `key = value` per line (`theme = light|dark|auto`, `layout = auto|inline|side-by-side`). It is read once at start-up and read anew before each save, so a value another moonhare saved earlier is kept; lines and keys moonhare does not know are kept as they are. A file that is not UTF-8, holds NUL or is larger than 64 KiB counts as empty at start-up, and an unknown value is ignored; such a file, or one that cannot be read, is never overwritten: the save fails. A save goes to a temporary file in the same directory, which is renamed over the old one; when the file is a symbolic link (as dotfile managers such as stow make), the link is followed and the temporary file is made beside its target, so the link stays. A failed save is a message in the status line; the choice still holds for the run.

## Priority

`--theme` > `MOONHARE_THEME` > the saved theme > dark. The flag and the variable hold for one run and are never saved; only `:theme` saves. The Layout has no flag: the saved choice, else by width (`SIDE_BY_SIDE_MIN_WIDTH`). The terminal is still asked for its background and for mode 2031 reports whatever the theme (ADR 0007): the replies change nothing unless the theme is `auto`, and `:theme auto` can then follow the background at once and its later changes.

## Considered Options

- Beside the Review in `.git/moonhare/`: per clone, so every new repository would start dark again.
- `git config --global moonhare.theme`: no new file, but moonhare would write the Reviewer's git config, and would have to run git to read it before the first frame.
- `$XDG_CONFIG_HOME`: config is what the user writes by hand; this is state the program writes, which the XDG spec puts under `$XDG_STATE_HOME`.
- TOML or JSON: a parser or more of `serde_json` for two keys; `key = value` reads and edits by hand as well.

## Consequences

- Session tests never touch the Reviewer's file: a `Session` saves nothing until `use_preferences` gives it a directory, and the pty tests set `XDG_STATE_HOME` to a temporary one.
- There is no lock. Two moonhare processes saving at nearly the same moment can lose one of the two saves, even of different keys: each reads the file, then renames its own copy over it, and a save that falls between the other's read and rename is gone. The window is the time of one small write, and the Reviewer only loses that one choice, which `:theme` or `s` makes again.
- The status line names the theme only when it is not the default (`theme:light`, `theme:auto`).
