# Read the terminal ourselves and take its replies out before crossterm's parser

`auto` follows the terminal background: moonhare asks for it at start-up (OSC 11) and asks the terminal to report changes of its colour scheme (DEC mode 2031, `CSI ? 997 ; 1|2 n`). crossterm 0.29 does not know these replies: its parser reads `ESC ]` as Alt-`]` and the rest as typed characters. Gluing a reply back together from those keys (tried in PR #74) broke whenever the reply did not look like one read: Esc and `]` joined by tmux's `escape-time`, late replies over ssh, replies cut into pieces, idle time counted against the pieces. So moonhare reads the terminal itself (`terminal_input`): a reader thread hands raw reads to a `Decoder`, which takes out the replies as `Reply` and gives every other byte to crossterm's own parser, vendored in `src/terminal_input/keys.rs` (MIT, its licence kept in the file header; it is private in crossterm, and one file is too small for `vendor/`). The rest of moonhare keeps crossterm's `Event`, unchanged; resizes come from `SIGWINCH` through `signal-hook`, as in crossterm.

## How a reply is told from keys

A reply starts with `ESC ] 11 ;` or `ESC [ ?`, which no key sends. Bytes that may still become such a start (`ESC`, `ESC ]`, `ESC ] 1`, `ESC ] 11`, `ESC [`) are held until the next byte decides; when it does not continue the start they are fed to the parser exactly as crossterm's reader would have fed them, including where reads ended, so `ESC ] h` in one read is Alt-`]` and `h`, and Esc read alone is Esc. Nothing comes held for longer than `ESC_GAP` (25 ms): a lone Esc is late by that much, which is what Neovim's `ttimeoutlen` does too, and much less than tmux's own `escape-time`. Once the start is certain, the reply is read to its end (BEL or ST, or the final byte of the CSI) however its pieces come, up to `REPLY_GAP` (1 s) between pieces; a reply that never ends, is too long, or has an unknown body is dropped, never typed. Inside a bracketed paste nothing is a reply. Held bytes time out only when nothing read after them is waiting: the reader thread stamps each read, and what queued up while the loop was busy drawing is taken in before any deadline is checked.

The replies change the theme only under `auto`; `--theme`, `MOONHARE_THEME` and `:theme light|dark` outrank them, and `:theme auto` asks for the background anew. At start-up moonhare also asks for the current scheme (`CSI ? 996 n`), answered like a 2031 report. Queries go out after raw mode is on, so an early reply is not echoed, and the first frame never waits. Mode 2031 is turned off on exit and in the panic hook.

## Considered Options

- Keep crossterm's reader and filter the keys it makes of a reply (PR #74): cannot tell Alt-`]` typed in tmux from a reply, and a reply cut after its Esc is gone before the filter sees it.
- termwiz as the input layer: knows OSC and DSR replies, but brings its own key, mouse and terminal types; ~900 uses of crossterm's event types in 21 files would move, and the screen would need ratatui's termwiz backend.
- termina (Helix's crossterm successor, which parses mode-2031 reports): its events differ from crossterm's too, ratatui has no backend for it, and output would stay on crossterm with two owners of one terminal.
- Our own key parser: more code to own than the vendored one, and it would read some keys differently from crossterm.

## Consequences

- We own a copy of crossterm's parser; when ratatui moves to a newer crossterm, compare `src/event/sys/unix/parse.rs` and update the copy if keys changed. crossterm's event reader must never be used (`event::poll`/`read`): two readers would split the input between them.
- Typing Esc, `]`, `1`, `1`, `;` within 25 ms of each other, or the same keys joined by tmux, is read as the start of a reply.
- A reply cut right after its leading Esc and delayed by more than 25 ms still becomes keys; over a network that is rare.
- Two Escs in one read are two Escs (crossterm made one): the second may start a reply.
- `signal-hook` becomes a direct dependency (it already came with crossterm) for `SIGWINCH`.
- A reply arriving after moonhare has left raw mode is echoed by the terminal; only a reply slower than the whole session can.
- Unix only, as moonhare is.
