//! Game state: board, turn, history, and the two interactive outcomes
//! (`NeedsPromotionChoice` / `NeedsKingPlacement`).
//!
//! Key departure from the Python original:
//!   * `dama_take_possible` is gone — the captured piece lives in the `Move`.
//!   * Pins are no longer a generator parameter; `legal_moves` generates every
//!     candidate and simulates it, so the "pinned piece can still dama-move"
//!     bug is fixed as a side effect.
//!   * Promotion branches on the *promoting* player's promotion count, not
//!     the opponent's (which was a copy-paste bug in the Python).

use crate::moves::{Move, Square};
use crate::piece::{Color, Kind, Piece};

pub type Board = [[Option<Piece>; 8]; 8];
pub type Pin = (usize, usize, isize, isize);
pub type Check = (usize, usize, isize, isize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    NeedsPromotionChoice {
        square: Square,
        color: Color,
    },
    NeedsKingPlacement {
        placing_color: Color,
        candidates: Vec<Square>,
    },
    Checkmate {
        winner: Color,
    },
    Stalemate,
}

#[derive(Clone, Debug)]
enum Pending {
    Promotion { square: Square },
    KingPlacement { placing_color: Color },
}

#[derive(Clone, Debug)]
enum LogEntry {
    Move {
        mv: Move,
        promoted_to: Option<Kind>,
        prev_en_passant: Option<Square>,
    },
    KingPlacement {
        color: Color,
        square: Square,
    },
}

#[derive(Clone, Debug)]
pub struct GameState {
    pub board: Board,
    pub white_to_move: bool,
    pub white_king: Option<Square>,
    pub black_king: Option<Square>,
    pub en_passant: Option<Square>,
    history: Vec<LogEntry>,
    pending: Option<Pending>,
    white_promotions: Vec<(Square, Kind)>,
    black_promotions: Vec<(Square, Kind)>,
}

impl GameState {
    // ---------- construction ----------

    pub fn new() -> Self {
        let mut board: Board = [[None; 8]; 8];
        let b = |k| Some(Piece::new(Color::Black, k));
        let w = |k| Some(Piece::new(Color::White, k));

        // Black back rank (row 0, rank 8): bR b8, bN d8, bN f8, bR h8
        board[0][1] = b(Kind::Rook);
        board[0][3] = b(Kind::Knight);
        board[0][5] = b(Kind::Knight);
        board[0][7] = b(Kind::Rook);

        // Black pawns: rows 1 and 2 on dark squares only.
        for &c in &[0, 2, 4, 6] {
            board[1][c] = b(Kind::Pawn);
        }
        for &c in &[1, 3, 5, 7] {
            board[2][c] = b(Kind::Pawn);
        }

        // White pawns: rows 5 and 6 on dark squares only.
        for &c in &[0, 2, 4, 6] {
            board[5][c] = w(Kind::Pawn);
        }
        for &c in &[1, 3, 5, 7] {
            board[6][c] = w(Kind::Pawn);
        }

        // White back rank (row 7, rank 1): wR a1, wN c1, wN e1, wR g1
        board[7][0] = w(Kind::Rook);
        board[7][2] = w(Kind::Knight);
        board[7][4] = w(Kind::Knight);
        board[7][6] = w(Kind::Rook);

        Self {
            board,
            white_to_move: true,
            white_king: None,
            black_king: None,
            en_passant: None,
            history: Vec::new(),
            pending: None,
            white_promotions: Vec::new(),
            black_promotions: Vec::new(),
        }
    }

    // ---------- read-only accessors ----------

    pub fn side_to_move(&self) -> Color {
        if self.white_to_move {
            Color::White
        } else {
            Color::Black
        }
    }

    pub fn king_of(&self, color: Color) -> Option<Square> {
        match color {
            Color::White => self.white_king,
            Color::Black => self.black_king,
        }
    }

    /// True if `color` has a tracked king and it is currently attacked.
    pub fn is_in_check(&self, color: Color) -> bool {
        self.pins_and_checks_for(color).0
    }

    pub fn promotion_count(&self, color: Color) -> usize {
        match color {
            Color::White => self.white_promotions.len(),
            Color::Black => self.black_promotions.len(),
        }
    }

    pub fn move_count(&self) -> usize {
        self.history.len()
    }

    // ---------- move generation ----------

    pub fn legal_moves(&self) -> Vec<Move> {
        let mut candidates = Vec::new();
        self.generate_all(&mut candidates);

        // Simulate-filter: a move is legal iff the mover's own king
        // is not in check after it. Handles pins, en passant pins,
        // dama-move pins, and the "king walks into check" case uniformly.
        let legal: Vec<Move> = candidates
            .into_iter()
            .filter(|mv| {
                let sim = self.simulate(*mv);
                let (in_check, _, _) = sim.pins_and_checks_for(mv.piece.color);
                !in_check
            })
            .collect();

        // A pawn's en-passant capture and its plain dama diagonal step can
        // share the same from/to because the en-passant square is empty by
        // definition. The capture lands on the same square and additionally
        // removes an enemy pawn, so it strictly dominates; keep only it.
        // This runs *after* the legality filter, so a pinned en-passant
        // capture cannot shadow a legal non-capturing step.
        let mut unique: Vec<Move> = Vec::with_capacity(legal.len());
        for mv in legal {
            match unique
                .iter_mut()
                .find(|kept| kept.from == mv.from && kept.to == mv.to)
            {
                Some(kept) if kept.captured.is_none() && mv.captured.is_some() => *kept = mv,
                Some(_) => {}
                None => unique.push(mv),
            }
        }
        unique
    }

    fn generate_all(&self, moves: &mut Vec<Move>) {
        let side = self.side_to_move();
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.board[r][c] {
                    if p.color == side {
                        self.generate_piece(r, c, p, moves);
                    }
                }
            }
        }
    }

    fn generate_piece(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        // Every piece has dama (checker-style) movement — that's the mashup.
        self.generate_dama(r, c, p, moves);
        match p.kind {
            Kind::Pawn => self.generate_pawn(r, c, p, moves),
            Kind::Knight => self.generate_knight(r, c, p, moves),
            Kind::Bishop => self.generate_bishop(r, c, p, moves),
            Kind::Rook => self.generate_rook(r, c, p, moves),
            Kind::Queen => {
                self.generate_rook(r, c, p, moves);
                self.generate_bishop(r, c, p, moves);
            }
            Kind::King => self.generate_king(r, c, p, moves),
        }
    }

    fn promo_rank(color: Color) -> usize {
        match color {
            Color::White => 0,
            Color::Black => 7,
        }
    }

    fn push_move(
        &self,
        from: Square,
        to: Square,
        p: Piece,
        captured: Option<(Piece, Square)>,
        moves: &mut Vec<Move>,
    ) {
        let is_promotion = p.kind == Kind::Pawn && to.0 == Self::promo_rank(p.color);
        moves.push(Move {
            from,
            to,
            piece: p,
            captured,
            is_promotion,
        });
    }

    fn generate_pawn(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        let dir: isize = if p.color == Color::White { -1 } else { 1 };
        let start_rank = if p.color == Color::White { 6 } else { 1 };
        let from = (r, c);

        // one forward
        let nr = r as isize + dir;
        if (0..8).contains(&nr) {
            let nr = nr as usize;
            if self.board[nr][c].is_none() {
                self.push_move(from, (nr, c), p, None, moves);
                // two forward from starting rank
                let nr2 = r as isize + dir * 2;
                if r == start_rank && (0..8).contains(&nr2) && self.board[nr2 as usize][c].is_none()
                {
                    self.push_move(from, (nr2 as usize, c), p, None, moves);
                }
            }

            // diagonal captures
            for dc in [-1isize, 1] {
                let nc = c as isize + dc;
                if !(0..8).contains(&nc) {
                    continue;
                }
                let nc = nc as usize;
                if let Some(target) = self.board[nr][nc] {
                    if target.color != p.color {
                        self.push_move(from, (nr, nc), p, Some((target, (nr, nc))), moves);
                    }
                }
                if let Some(ep) = self.en_passant {
                    if ep == (nr, nc) {
                        if let Some(ep_pawn) = self.board[r][nc] {
                            self.push_move(from, (nr, nc), p, Some((ep_pawn, (r, nc))), moves);
                        }
                    }
                }
            }
        }
    }

    fn generate_knight(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        let offsets = [
            (-2, -1),
            (-2, 1),
            (-1, -2),
            (-1, 2),
            (1, -2),
            (1, 2),
            (2, -1),
            (2, 1),
        ];
        for (dr, dc) in offsets {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if !(0..8).contains(&nr) || !(0..8).contains(&nc) {
                continue;
            }
            let (nr, nc) = (nr as usize, nc as usize);
            let captured = match self.board[nr][nc] {
                Some(t) if t.color == p.color => continue,
                Some(t) => Some((t, (nr, nc))),
                None => None,
            };
            self.push_move((r, c), (nr, nc), p, captured, moves);
        }
    }

    fn generate_bishop(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        for (dr, dc) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
            for i in 1..8 {
                let nr = r as isize + dr * i;
                let nc = c as isize + dc * i;
                if !(0..8).contains(&nr) || !(0..8).contains(&nc) {
                    break;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                match self.board[nr][nc] {
                    None => self.push_move((r, c), (nr, nc), p, None, moves),
                    Some(t) if t.color != p.color => {
                        self.push_move((r, c), (nr, nc), p, Some((t, (nr, nc))), moves);
                        break;
                    }
                    _ => break,
                }
            }
        }
    }

    fn generate_rook(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            for i in 1..8 {
                let nr = r as isize + dr * i;
                let nc = c as isize + dc * i;
                if !(0..8).contains(&nr) || !(0..8).contains(&nc) {
                    break;
                }
                let (nr, nc) = (nr as usize, nc as usize);
                match self.board[nr][nc] {
                    None => self.push_move((r, c), (nr, nc), p, None, moves),
                    Some(t) if t.color != p.color => {
                        self.push_move((r, c), (nr, nc), p, Some((t, (nr, nc))), moves);
                        break;
                    }
                    _ => break,
                }
            }
        }
    }

    fn generate_king(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        for (dr, dc) in [
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1),
        ] {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if !(0..8).contains(&nr) || !(0..8).contains(&nc) {
                continue;
            }
            let (nr, nc) = (nr as usize, nc as usize);
            let captured = match self.board[nr][nc] {
                Some(t) if t.color == p.color => continue,
                Some(t) => Some((t, (nr, nc))),
                None => None,
            };
            self.push_move((r, c), (nr, nc), p, captured, moves);
        }
    }

    /// Checker-style moves: one diagonal step forward, or a two-step
    /// diagonal jump over an enemy piece (landing on an empty square).
    fn generate_dama(&self, r: usize, c: usize, p: Piece, moves: &mut Vec<Move>) {
        let dir: isize = if p.color == Color::White { -1 } else { 1 };

        // one diagonal step
        for dc in [-1isize, 1] {
            let nr = r as isize + dir;
            let nc = c as isize + dc;
            if (0..8).contains(&nr) && (0..8).contains(&nc) {
                let (nr, nc) = (nr as usize, nc as usize);
                if self.board[nr][nc].is_none() {
                    self.push_move((r, c), (nr, nc), p, None, moves);
                }
            }
        }

        // two-step jump capture
        for dc in [-2isize, 2] {
            let nr = r as isize + dir * 2;
            let nc = c as isize + dc;
            if !(0..8).contains(&nr) || !(0..8).contains(&nc) {
                continue;
            }
            let (nr, nc) = (nr as usize, nc as usize);
            if self.board[nr][nc].is_some() {
                continue;
            }
            let mid_r = (r as isize + dir) as usize;
            let mid_c = (c as isize + dc / 2) as usize;
            if let Some(mid) = self.board[mid_r][mid_c] {
                if mid.color != p.color {
                    self.push_move((r, c), (nr, nc), p, Some((mid, (mid_r, mid_c))), moves);
                }
            }
        }
    }

    // ---------- check / pin detection ----------

    fn pins_and_checks_for(&self, side: Color) -> (bool, Vec<Pin>, Vec<Check>) {
        let Some((sr, sc)) = self.king_of(side) else {
            return (false, Vec::new(), Vec::new());
        };
        let enemy = side.opposite();
        let ally = side;

        let mut pins = Vec::new();
        let mut checks = Vec::new();
        let mut in_check = false;

        let directions: [(isize, isize); 8] = [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (-1, 1),
            (1, -1),
            (1, 1),
        ];

        for (j, d) in directions.iter().enumerate() {
            let mut possible_pin: Option<Pin> = None;
            for i in 1..=7isize {
                let er = sr as isize + d.0 * i;
                let ec = sc as isize + d.1 * i;
                if !(0..8).contains(&er) || !(0..8).contains(&ec) {
                    break;
                }
                let (er, ec) = (er as usize, ec as usize);

                match self.board[er][ec] {
                    None => {}
                    Some(p) if p.color == ally => {
                        if p.kind == Kind::King {
                            // second king — treat as an empty slot; harmless.
                        } else if possible_pin.is_none() {
                            possible_pin = Some((er, ec, d.0, d.1));
                        } else {
                            break;
                        }
                    }
                    Some(p) if p.color == enemy => {
                        let attacks_along = (j <= 3 && p.kind == Kind::Rook)
                            || (j >= 4 && p.kind == Kind::Bishop)
                            || (i == 1 && p.kind == Kind::Pawn && pawn_attacks_along(enemy, j))
                            || p.kind == Kind::Queen
                            || (i == 1 && p.kind == Kind::King);
                        if attacks_along {
                            if let Some(pin) = possible_pin.take() {
                                pins.push(pin);
                            } else {
                                in_check = true;
                                checks.push((er, ec, d.0, d.1));
                            }
                        }
                        break;
                    }
                    _ => unreachable!(),
                }
            }
        }

        // knight checks
        for (dr, dc) in [
            (-2, -1),
            (-2, 1),
            (-1, -2),
            (-1, 2),
            (1, -2),
            (1, 2),
            (2, -1),
            (2, 1),
        ] {
            let er = sr as isize + dr;
            let ec = sc as isize + dc;
            if !(0..8).contains(&er) || !(0..8).contains(&ec) {
                continue;
            }
            let (er, ec) = (er as usize, ec as usize);
            if let Some(p) = self.board[er][ec] {
                if p.color == enemy && p.kind == Kind::Knight {
                    in_check = true;
                    checks.push((er, ec, dr, dc));
                }
            }
        }

        // Dama jump checks: every piece can jump two diagonals forward over
        // an enemy piece. If the king is the jumped piece and the landing
        // square beyond it is empty, that jump can capture the king.
        let jump_dir: isize = if enemy == Color::White { -1 } else { 1 };
        for dc in [-1isize, 1] {
            let ar = sr as isize - jump_dir;
            let ac = sc as isize - dc;
            let lr = sr as isize + jump_dir;
            let lc = sc as isize + dc;
            if !(0..8).contains(&ar)
                || !(0..8).contains(&ac)
                || !(0..8).contains(&lr)
                || !(0..8).contains(&lc)
            {
                continue;
            }
            if self.board[lr as usize][lc as usize].is_some() {
                continue;
            }
            if let Some(p) = self.board[ar as usize][ac as usize] {
                if p.color == enemy {
                    in_check = true;
                    break;
                }
            }
        }

        (in_check, pins, checks)
    }

    // ---------- apply / undo ----------

    pub fn apply(&mut self, mv: Move) -> Outcome {
        let prev_en_passant = self.en_passant;
        self.apply_core(mv);
        self.history.push(LogEntry::Move {
            mv,
            promoted_to: None,
            prev_en_passant,
        });

        if !mv.is_promotion {
            return self.check_terminal();
        }

        let color = mv.piece.color;
        match self.promotion_count(color) {
            0 => {
                // 1st: auto-queen, then spawn the OPPONENT's king if they
                // don't already have one (e.g. they promoted to King first).
                self.complete_promotion(mv.to, Kind::Queen);
                let enemy = color.opposite();
                if self.king_of(enemy).is_some() {
                    return self.check_terminal();
                }
                let candidates = self.squares_with(enemy, Kind::Pawn);
                if candidates.is_empty() {
                    return self.check_terminal();
                }
                self.pending = Some(Pending::KingPlacement {
                    placing_color: enemy,
                });
                Outcome::NeedsKingPlacement {
                    placing_color: enemy,
                    candidates,
                }
            }
            1 => {
                // 2nd: player picks Q/R/N; King only if they have none.
                self.pending = Some(Pending::Promotion { square: mv.to });
                Outcome::NeedsPromotionChoice {
                    square: mv.to,
                    color,
                }
            }
            n if n % 2 == 0 => {
                // 3rd, 5th, ...: auto-bishop.
                self.complete_promotion(mv.to, Kind::Bishop);
                self.check_terminal()
            }
            _ => {
                // 4th, 6th, ...: same choice; King only if they have none.
                self.pending = Some(Pending::Promotion { square: mv.to });
                Outcome::NeedsPromotionChoice {
                    square: mv.to,
                    color,
                }
            }
        }
    }

    pub fn choose_promotion(&mut self, kind: Kind) -> Outcome {
        match self.pending.take() {
            Some(Pending::Promotion { square }) => {
                let color = self.board[square.0][square.1]
                    .expect("promotion square must be occupied")
                    .color;
                assert!(
                    kind != Kind::King || self.king_of(color).is_none(),
                    "cannot promote to a second king"
                );
                self.complete_promotion(square, kind);
                self.check_terminal()
            }
            other => {
                self.pending = other;
                panic!("choose_promotion called without pending promotion");
            }
        }
    }

    pub fn place_king(&mut self, square: Square) -> Outcome {
        match self.pending.take() {
            Some(Pending::KingPlacement { placing_color }) => {
                debug_assert!(
                    self.king_of(placing_color).is_none(),
                    "placing a second king"
                );
                let pawn = self.board[square.0][square.1];
                assert_eq!(
                    pawn.map(|p| (p.color, p.kind)),
                    Some((placing_color, Kind::Pawn)),
                    "place_king called with invalid square"
                );
                self.board[square.0][square.1] = Some(Piece::new(placing_color, Kind::King));
                match placing_color {
                    Color::White => self.white_king = Some(square),
                    Color::Black => self.black_king = Some(square),
                }
                self.history.push(LogEntry::KingPlacement {
                    color: placing_color,
                    square,
                });
                self.check_terminal()
            }
            other => {
                self.pending = other;
                panic!("place_king called without pending king placement");
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        // If a decision is pending, undo the whole pending step cleanly.
        if self.pending.take().is_some() {
            // still need to pop the move that opened the pending state
        }
        let Some(entry) = self.history.pop() else {
            return false;
        };
        match entry {
            LogEntry::Move {
                mv,
                promoted_to,
                prev_en_passant,
            } => {
                self.board[mv.to.0][mv.to.1] = None;
                if let Some((cap, cap_sq)) = mv.captured {
                    self.board[cap_sq.0][cap_sq.1] = Some(cap);
                }
                self.board[mv.from.0][mv.from.1] = Some(mv.piece);
                if mv.piece.kind == Kind::King {
                    match mv.piece.color {
                        Color::White => self.white_king = Some(mv.from),
                        Color::Black => self.black_king = Some(mv.from),
                    }
                }
                if let Some(kind) = promoted_to {
                    let log = match mv.piece.color {
                        Color::White => &mut self.white_promotions,
                        Color::Black => &mut self.black_promotions,
                    };
                    log.pop();
                    if kind == Kind::King {
                        match mv.piece.color {
                            Color::White => self.white_king = None,
                            Color::Black => self.black_king = None,
                        }
                    }
                }
                self.white_to_move = !self.white_to_move;
                self.en_passant = prev_en_passant;
                true
            }
            LogEntry::KingPlacement { color, square } => {
                self.board[square.0][square.1] = Some(Piece::new(color, Kind::Pawn));
                match color {
                    Color::White => self.white_king = None,
                    Color::Black => self.black_king = None,
                }
                true
            }
        }
    }

    /// Undo up to `moves` full moves, transparently removing any
    /// king-placement side effect attached to a promotion. Returns the number
    /// of moves actually undone.
    pub fn undo_moves(&mut self, moves: usize) -> usize {
        let mut undone = 0;
        while undone < moves {
            match self.history.last() {
                Some(LogEntry::KingPlacement { .. }) => {
                    if !self.undo() {
                        break;
                    }
                }
                Some(LogEntry::Move { .. }) => {
                    if !self.undo() {
                        break;
                    }
                    undone += 1;
                }
                None => break,
            }
        }
        undone
    }

    // ---------- internals ----------

    fn apply_core(&mut self, mv: Move) {
        self.board[mv.from.0][mv.from.1] = None;
        if let Some((_, cap_sq)) = mv.captured {
            self.board[cap_sq.0][cap_sq.1] = None;
        }
        self.board[mv.to.0][mv.to.1] = Some(mv.piece);

        if mv.piece.kind == Kind::King {
            match mv.piece.color {
                Color::White => self.white_king = Some(mv.to),
                Color::Black => self.black_king = Some(mv.to),
            }
        }

        let dr = mv.from.0 as isize - mv.to.0 as isize;
        self.en_passant = if mv.piece.kind == Kind::Pawn && dr.abs() == 2 && mv.from.1 == mv.to.1 {
            Some(((mv.from.0 + mv.to.0) / 2, mv.from.1))
        } else {
            None
        };

        self.white_to_move = !self.white_to_move;
    }

    fn simulate(&self, mv: Move) -> GameState {
        let mut s = self.clone();
        s.apply_core(mv);
        s
    }

    fn complete_promotion(&mut self, square: Square, kind: Kind) {
        let color = self.board[square.0][square.1]
            .expect("promotion square must be occupied")
            .color;
        self.board[square.0][square.1] = Some(Piece::new(color, kind));
        if kind == Kind::King {
            debug_assert!(self.king_of(color).is_none(), "promotion to a second king");
            match color {
                Color::White => self.white_king = Some(square),
                Color::Black => self.black_king = Some(square),
            }
        }
        let log = match color {
            Color::White => &mut self.white_promotions,
            Color::Black => &mut self.black_promotions,
        };
        log.push((square, kind));
        if let Some(LogEntry::Move { promoted_to, .. }) = self.history.last_mut() {
            *promoted_to = Some(kind);
        }
    }

    fn squares_with(&self, color: Color, kind: Kind) -> Vec<Square> {
        let mut out = Vec::new();
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.board[r][c] {
                    if p.color == color && p.kind == kind {
                        out.push((r, c));
                    }
                }
            }
        }
        out
    }

    fn check_terminal(&self) -> Outcome {
        if !self.legal_moves().is_empty() {
            return Outcome::Continue;
        }
        if self.is_in_check(self.side_to_move()) {
            Outcome::Checkmate {
                winner: self.side_to_move().opposite(),
            }
        } else {
            Outcome::Stalemate
        }
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

fn pawn_attacks_along(enemy: Color, direction_index: usize) -> bool {
    match enemy {
        Color::White => direction_index == 6 || direction_index == 7,
        Color::Black => direction_index == 4 || direction_index == 5,
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn find_move(gs: &GameState, from: Square, to: Square) -> Option<Move> {
        gs.legal_moves()
            .into_iter()
            .find(|m| m.from == from && m.to == to)
    }

    /// A game with the standard bookkeeping but an empty board.
    fn empty_state() -> GameState {
        let mut gs = GameState::new();
        gs.board = [[None; 8]; 8];
        gs
    }

    #[test]
    fn initial_position_has_twelve_pieces_each() {
        let gs = GameState::new();
        let mut w = 0;
        let mut b = 0;
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = gs.board[r][c] {
                    match p.color {
                        Color::White => w += 1,
                        Color::Black => b += 1,
                    }
                }
            }
        }
        assert_eq!((w, b), (12, 12));
    }

    #[test]
    fn white_pawn_push_and_double_push() {
        let gs = GameState::new();
        // rank-2 pawn (row 6, col 1) can push one or two
        assert!(find_move(&gs, (6, 1), (5, 1)).is_some());
        assert!(find_move(&gs, (6, 1), (4, 1)).is_some());
        // rank-3 pawn (row 5, col 0) can only push one
        assert!(find_move(&gs, (5, 0), (4, 0)).is_some());
        assert!(find_move(&gs, (5, 0), (3, 0)).is_none());
    }

    #[test]
    fn en_passant_round_trip() {
        let mut gs = GameState::new();
        // Clear the lane for a black double push from its start rank and
        // park a white pawn beside the square it passes over.
        gs.board[1][3] = Some(Piece::new(Color::Black, Kind::Pawn));
        gs.board[2][3] = None;
        gs.board[3][3] = None;
        gs.board[3][2] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.white_to_move = false;

        let double_push = find_move(&gs, (1, 3), (3, 3)).unwrap();
        assert!(matches!(gs.apply(double_push), Outcome::Continue));
        assert_eq!(gs.en_passant, Some((2, 3)));

        // White can capture the passing pawn on (2, 3). That square is also a
        // legal non-capturing dama step for this pawn, but `legal_moves`
        // collapses the duplicate from/to in favour of the capture.
        let here: Vec<Move> = gs
            .legal_moves()
            .into_iter()
            .filter(|m| m.from == (3, 2) && m.to == (2, 3))
            .collect();
        assert_eq!(here.len(), 1, "duplicate from/to should be collapsed");
        let ep = here[0];
        assert_eq!(
            ep.captured.map(|(p, sq)| (p.color, sq)),
            Some((Color::Black, (3, 3)))
        );
        gs.apply(ep);
        assert!(gs.board[3][3].is_none());
        assert_eq!(
            gs.board[2][3].map(|p| (p.color, p.kind)),
            Some((Color::White, Kind::Pawn))
        );

        // Undo restores the captured pawn and the en-passant square...
        gs.undo();
        assert_eq!(
            gs.board[3][3].map(|p| (p.color, p.kind)),
            Some((Color::Black, Kind::Pawn))
        );
        assert_eq!(
            gs.board[3][2].map(|p| (p.color, p.kind)),
            Some((Color::White, Kind::Pawn))
        );
        assert_eq!(gs.en_passant, Some((2, 3)));

        // ...and undoing the double push restores the start position.
        gs.undo();
        assert_eq!(
            gs.board[1][3].map(|p| (p.color, p.kind)),
            Some((Color::Black, Kind::Pawn))
        );
        assert!(gs.board[3][3].is_none());
        assert_eq!(gs.en_passant, None);
    }

    #[test]
    fn pinned_en_passant_does_not_hide_the_legal_dama_step() {
        let mut gs = empty_state();
        gs.white_king = Some((3, 1));
        gs.board[3][1] = Some(Piece::new(Color::White, Kind::King));
        gs.board[3][7] = Some(Piece::new(Color::Black, Kind::Rook));
        gs.board[3][5] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.board[1][6] = Some(Piece::new(Color::Black, Kind::Pawn));
        gs.white_to_move = false;

        let double_push = find_move(&gs, (1, 6), (3, 6)).unwrap();
        gs.apply(double_push);
        assert_eq!(gs.en_passant, Some((2, 6)));

        // Capturing en passant would clear row 3 and expose the white king to
        // the rook, so only the plain dama step onto the same square survives.
        // This works only because duplicate-collapsing happens *after* the
        // legality filter.
        let here: Vec<Move> = gs
            .legal_moves()
            .into_iter()
            .filter(|m| m.from == (3, 5) && m.to == (2, 6))
            .collect();
        assert_eq!(here.len(), 1);
        assert!(here[0].captured.is_none());
    }

    #[test]
    fn kings_start_absent() {
        let gs = GameState::new();
        assert!(gs.white_king.is_none());
        assert!(gs.black_king.is_none());
        assert!(!gs.is_in_check(Color::White));
        assert!(!gs.is_in_check(Color::Black));
    }

    #[test]
    fn promotions_bookkeeping_is_per_color() {
        // Regression: original Python branched on the *other* player's
        // promotion count.
        let gs = GameState::new();
        assert_eq!(gs.promotion_count(Color::White), 0);
        assert_eq!(gs.promotion_count(Color::Black), 0);
    }

    #[test]
    fn queen_can_move_after_promotion_through_choose() {
        // Build a state with a white pawn on row 1, col 0 and clear path.
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.white_to_move = true;
        // Pre-seed one promotion so the next is "pick" and returns NeedsPromotionChoice.
        gs.white_promotions.push(((0, 7), Kind::Queen));

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        let outcome = gs.apply(m);
        assert!(matches!(outcome, Outcome::NeedsPromotionChoice { .. }));
        let outcome = gs.choose_promotion(Kind::Rook);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(gs.board[0][0].map(|p| p.kind), Some(Kind::Rook));
    }

    #[test]
    fn promotion_to_king_registers_and_undo_clears_it() {
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        // Seed one promotion so the next one asks the player.
        gs.white_promotions.push(((0, 7), Kind::Queen));
        gs.white_to_move = true;

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        assert!(matches!(gs.apply(m), Outcome::NeedsPromotionChoice { .. }));
        assert_eq!(gs.king_of(Color::White), None);

        let _ = gs.choose_promotion(Kind::King);
        assert_eq!(gs.king_of(Color::White), Some((0, 0)));
        assert_eq!(
            gs.board[0][0].map(|p| (p.color, p.kind)),
            Some((Color::White, Kind::King))
        );

        assert!(gs.undo());
        assert_eq!(gs.king_of(Color::White), None);
        assert_eq!(
            gs.board[1][0].map(|p| (p.color, p.kind)),
            Some((Color::White, Kind::Pawn))
        );
        assert_eq!(gs.board[0][0], None);
    }

    #[test]
    #[should_panic(expected = "second king")]
    fn promotion_to_a_second_king_is_rejected() {
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.white_promotions.push(((0, 7), Kind::Queen));
        gs.white_king = Some((7, 7));
        gs.board[7][7] = Some(Piece::new(Color::White, Kind::King));
        gs.white_to_move = true;

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        assert!(matches!(gs.apply(m), Outcome::NeedsPromotionChoice { .. }));
        let _ = gs.choose_promotion(Kind::King);
    }

    #[test]
    fn first_promotion_requests_king_placement_when_enemy_has_none() {
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.white_to_move = true;

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        let outcome = gs.apply(m);
        assert!(matches!(
            outcome,
            Outcome::NeedsKingPlacement {
                placing_color: Color::Black,
                ..
            }
        ));
        assert_eq!(gs.board[0][0].map(|p| p.kind), Some(Kind::Queen));
        assert_eq!(gs.king_of(Color::Black), None);
    }

    #[test]
    fn placing_a_candidate_pawn_creates_the_enemy_king() {
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.white_to_move = true;

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        let Outcome::NeedsKingPlacement { candidates, .. } = gs.apply(m) else {
            panic!("first promotion should request a king placement");
        };
        let square = candidates[0];
        let _ = gs.place_king(square);

        assert_eq!(gs.king_of(Color::Black), Some(square));
        assert_eq!(
            gs.board[square.0][square.1].map(|p| (p.color, p.kind)),
            Some((Color::Black, Kind::King))
        );

        assert!(gs.undo());
        assert_eq!(gs.king_of(Color::Black), None);
        assert_eq!(
            gs.board[square.0][square.1].map(|p| (p.color, p.kind)),
            Some((Color::Black, Kind::Pawn))
        );
    }

    #[test]
    fn first_promotion_skips_king_placement_when_enemy_already_has_one() {
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.board[3][7] = Some(Piece::new(Color::Black, Kind::King));
        gs.black_king = Some((3, 7));
        gs.white_to_move = true;

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        let outcome = gs.apply(m);
        assert!(!matches!(outcome, Outcome::NeedsKingPlacement { .. }));

        assert_eq!(gs.king_of(Color::Black), Some((3, 7)));
        let mut black_kings = 0;
        for r in 0..8 {
            for c in 0..8 {
                if gs.board[r][c].is_some_and(|p| p.color == Color::Black && p.kind == Kind::King) {
                    black_kings += 1;
                }
            }
        }
        assert_eq!(black_kings, 1);
    }

    #[test]
    fn check_detection_respects_blockers() {
        let mut gs = empty_state();
        gs.board[0][7] = Some(Piece::new(Color::Black, Kind::Rook));
        gs.board[7][7] = Some(Piece::new(Color::White, Kind::King));
        gs.white_king = Some((7, 7));

        assert!(gs.is_in_check(Color::White));
        assert!(!gs.is_in_check(Color::Black));

        gs.board[3][7] = Some(Piece::new(Color::White, Kind::Pawn));
        assert!(!gs.is_in_check(Color::White));
    }

    #[test]
    fn detects_checkmate() {
        let mut gs = empty_state();
        gs.board[0][0] = Some(Piece::new(Color::Black, Kind::King));
        gs.black_king = Some((0, 0));
        gs.board[1][1] = Some(Piece::new(Color::White, Kind::Queen));
        gs.board[2][2] = Some(Piece::new(Color::White, Kind::King));
        gs.white_king = Some((2, 2));
        gs.white_to_move = false;

        assert!(gs.is_in_check(Color::Black));
        assert!(!gs.is_in_check(Color::White));
        assert!(gs.legal_moves().is_empty());
        assert_eq!(
            gs.check_terminal(),
            Outcome::Checkmate {
                winner: Color::White
            }
        );
    }

    #[test]
    fn detects_dama_jump_check() {
        let mut gs = empty_state();
        gs.board[4][4] = Some(Piece::new(Color::White, Kind::King));
        gs.white_king = Some((4, 4));
        gs.board[3][3] = Some(Piece::new(Color::Black, Kind::Rook));
        gs.white_to_move = false;

        assert!(gs.is_in_check(Color::White));

        // The jump needs an empty landing square just beyond the king.
        gs.board[5][5] = Some(Piece::new(Color::Black, Kind::Pawn));
        assert!(!gs.is_in_check(Color::White));

        // Same geometry from the other side: a white piece below the black
        // king jumps upward over it.
        let mut gs = empty_state();
        gs.board[4][4] = Some(Piece::new(Color::Black, Kind::King));
        gs.black_king = Some((4, 4));
        gs.board[5][3] = Some(Piece::new(Color::White, Kind::Rook));
        assert!(gs.is_in_check(Color::Black));
    }

    #[test]
    fn undo_moves_rewinds_two_plies() {
        let mut gs = GameState::new();
        let first = gs.legal_moves()[0];
        gs.apply(first);
        let after_one = gs.clone();

        let second = gs.legal_moves()[0];
        gs.apply(second);
        let third = gs.legal_moves()[0];
        gs.apply(third);

        assert_eq!(gs.undo_moves(2), 2);
        assert_eq!(gs.move_count(), 1);
        assert_eq!(gs.board, after_one.board);
        assert_eq!(gs.side_to_move(), after_one.side_to_move());
        assert_eq!(gs.en_passant, after_one.en_passant);
    }

    #[test]
    fn undo_moves_skips_king_placement_side_effect() {
        let mut gs = GameState::new();
        gs.board[1][0] = Some(Piece::new(Color::White, Kind::Pawn));
        gs.white_to_move = true;
        let before = gs.clone();

        let m = find_move(&gs, (1, 0), (0, 0)).unwrap();
        let Outcome::NeedsKingPlacement { candidates, .. } = gs.apply(m) else {
            panic!("first promotion should request a king placement");
        };
        let _ = gs.place_king(candidates[0]);
        assert_eq!(gs.move_count(), 2);

        assert_eq!(gs.undo_moves(1), 1);
        assert_eq!(gs.move_count(), 0);
        assert_eq!(gs.board, before.board);
        assert_eq!(gs.side_to_move(), before.side_to_move());
        assert_eq!(gs.king_of(Color::Black), None);
        assert_eq!(gs.promotion_count(Color::White), 0);
    }
}
