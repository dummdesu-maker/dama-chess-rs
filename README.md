# Dama-Chess (Rust port)

A Rust port of [marckoxx/Dama-Chess](https://github.com/marckoxx/Dama-Chess) — a mashup of chess and checkers (dama), with the interactive terminal prompts replaced by in-window dialogs.

Original work © 2020-Present Marcko Pablo, Apache-2.0.

## Layout

```

dama-chess/
├── Cargo.toml          # workspace
├── dama-core/          # pure rules — no I/O, no deps
│   └── src/
│       ├── piece.rs
│       ├── moves.rs
│       └── state.rs
└── dama-ui/            # eframe + egui front-end
└── src/main.rs

```

## Build & run

```sh
cargo run --release -p dama-ui
```

First build pulls `eframe`/`egui` (~3 min). Subsequent builds are fast.

## Controls

| Key / Action ↕▾ | Effect ↕▾ |
|---|---|
| −Click a piece, then a highlighted square | Move |
| Click a highlighted pawn | Choose the opponent's king after your first promotion |
| Checkmate / stalemate dialog | Restart game, or undo the last 2 moves |
| Z | Undo one entry (move, or promotion side-effect) |
| R | New game |
| Esc | Quit |
⚙

While a piece is selected, empty destinations get a small green dot and every
piece that move would capture gets a green ring — including the enemy piece a
**dama jump hops over** (it sits between the piece and the landing dot) and a
pawn taken **en passant**.

Both captures land on an *empty* square, so either square can be clicked:
the landing dot, or the ringed piece itself. Clicking the ringed piece plays
the capture that claims it (when exactly one move does), which matches the
chess-capture habit of clicking the piece you want to take.

## Rules (as ported)

- Starting position has only **rooks, knights, and pawns** on both sides — no king, no queen, no bishop.
- Every piece can **also** make checker-style moves: one diagonal step forward, or a two-diagonal-step jump over an enemy piece to an empty landing square. That is the "dama" part.
- Pawns move/capture like chess pawns, plus dama moves. En passant is supported. No castling.
- **Promotion branches on how many times *you* have already promoted**:

1. 1st promotion — pawn becomes a **Queen**, and **you click one of your opponent's highlighted pawns to become *their* King** (skipped if they already have one).
2. 2nd, 4th, 6th, … — you pick **K / Q / R / N** (King only if you don't already have one; no bishop, no pawn).
3. 3rd, 5th, 7th, … — pawn becomes a **Bishop**.
- Check / checkmate only fires for a side that **has a king** on the board.
- Check covers both standard chess attacks and **dama jumps**: an enemy piece that could jump over the king to an empty landing square beyond it is giving check.
- A king in check is highlighted in **red**. Checkmate or stalemate opens a dialog with **Restart game** and **Undo 2 moves** (the latter also removes a promotion's king-placement side effect).

## Behaviour differences from the Python original

Porting exposed several bugs in the source; these are fixed:

| Bug | Fix |
|---|---|
| `see_checks_and_pins` read `white_king_location[1]` in the black branch | uses the correct colour's king col |
| Pins were pushed even when the pinner wasn't actually attacking along the ray | only real attackers pin |
| Pinned pieces could still make dama moves that broke the pin | every candidate move is now simulated — pin correctness is automatic |
| `undo` for the first-promotion king placement restored the wrong pawn colour | clean `LogEntry::KingPlacement` history |
| `undo` never reset the `first_white_dama` / `first_black_dama` flags | flags gone; history holds the truth |
| `if promoted_piece != 'B' or 'P':` was always true | modal only offers legal choices |
| En-passant `piece_captured` read after the square was cleared | captured piece is stored on the `Move` at generation time |
| Promotion branches checked the *other* player's promotion count | checks the promoting player's count |
| `highlight_enemy_pawns` crashed (`pygame.Surface` with two ints, `blit` with three args) | replaced with on-board pawn highlighting and click-to-choose |

## Assets

The UI draws pieces as Unicode glyphs (`♚ ♛ ♜ ♝ ♞ ♟`) tinted white / black with a drop shadow — no image files required, so the binary is self-contained.

If you'd prefer PNG sprites, replace the `ui.painter().text(...)` block in `dama-ui/src/main.rs::draw_board` with `ui.image(...)` pointing at `include_bytes!` textures.

## Testing

```
cargo test --workspace
```

Eighteen `dama-core` tests cover piece counts, pawn pushes, en passant from both colours (including the pinned case), check detection with blockers, dama-jump checks, checkmate detection, king absence at start, per-colour promotion bookkeeping, the interactive promotion round-trip, king-promotion tracking (registration, undo, rejection of a second king, and both branches of the first-promotion king placement), placing/undoing an enemy king from a highlighted pawn, and two-move undo (including skipping a king-placement side effect).

Seven `dama-ui` tests cover the board highlights and click handling. Two of them repaint the board into an offscreen `egui::Context` and inspect the emitted shapes, so the assertions are about pixels-to-be rather than about state: a dama take must paint a green ring centred on the jumped piece and a dot on the empty landing square, and a chess capture must keep its ring on the destination square. The rest drive `handle_click` end to end — en passant played through the landing square and through the ringed pawn, and a dama take played by clicking the jumped piece.

## License

Apache-2.0 — see original repository for copyright notice.

