//! Dama-Chess — egui front-end.
//!
//! The rules live entirely in `dama-core`; this crate only:
//!   1. draws the 8x8 board,
//!   2. translates clicks into `GameState::apply`,
//!   3. shows the promotion modal and handles king placement on the board.

use dama_core::{Color, GameState, Kind, Move, Outcome, Square};
use eframe::egui;

// ---------- UI phase ----------

enum Phase {
    Playing {
        selected: Option<Square>,
        legal: Vec<Move>,
    },
    Promotion {
        square: Square,
        color: Color,
    },
    KingPlacement {
        placing_color: Color,
        candidates: Vec<Square>,
    },
    GameOver {
        message: String,
        checkmate: bool,
    },
}

// ---------- App ----------

struct DamaApp {
    state: GameState,
    phase: Phase,
    /// The most recent move, highlighted on the board.
    last_move: Option<(Square, Square)>,
}

/// What the board should highlight for the current selection:
/// `(destinations, enemy squares that would be captured)`.
///
/// The capture squares are read from `Move::captured` rather than inferred
/// from "the destination is occupied". That inference only holds for chess
/// captures, where the victim stands on the destination; a dama jump lands on
/// the empty square *beyond* its victim, and en passant lands diagonally in
/// front of it.
fn selection_highlights(legal: &[Move], selected: Option<Square>) -> (Vec<Square>, Vec<Square>) {
    let mut targets = Vec::new();
    let mut captures = Vec::new();
    let Some(from) = selected else {
        return (targets, captures);
    };
    for mv in legal.iter().filter(|m| m.from == from) {
        targets.push(mv.to);
        if let Some((_, victim)) = mv.captured {
            captures.push(victim);
        }
    }
    (targets, captures)
}

impl DamaApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_state(GameState::new())
    }

    /// Split out from [`Self::new`] so tests can build an app from an arbitrary
    /// position without an `eframe::CreationContext`.
    fn with_state(state: GameState) -> Self {
        let legal = state.legal_moves();
        Self {
            state,
            phase: Phase::Playing {
                selected: None,
                legal,
            },
            last_move: None,
        }
    }

    fn handle_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Continue => self.enter_playing(),
            Outcome::NeedsPromotionChoice { square, color } => {
                self.phase = Phase::Promotion { square, color };
            }
            Outcome::NeedsKingPlacement {
                placing_color,
                candidates,
            } => {
                self.phase = Phase::KingPlacement {
                    placing_color,
                    candidates,
                };
            }
            Outcome::Checkmate { winner } => {
                let name = match winner {
                    Color::White => "White",
                    Color::Black => "Black",
                };
                self.phase = Phase::GameOver {
                    message: format!("{name} wins"),
                    checkmate: true,
                };
            }
            Outcome::Stalemate => {
                self.phase = Phase::GameOver {
                    message: "Draw — no legal moves".into(),
                    checkmate: false,
                };
            }
        }
    }

    fn enter_playing(&mut self) {
        let legal = self.state.legal_moves();
        self.phase = Phase::Playing {
            selected: None,
            legal,
        };
    }

    fn reset(&mut self) {
        self.state = GameState::new();
        self.last_move = None;
        self.enter_playing();
    }

    fn undo(&mut self) {
        if matches!(
            self.phase,
            Phase::Promotion { .. } | Phase::KingPlacement { .. }
        ) {
            return;
        }
        if self.state.undo() {
            self.last_move = None;
            self.enter_playing();
        }
    }

    fn undo_moves(&mut self, moves: usize) {
        // Reached from the game-over dialog. The core skips king-placement
        // side effects, so this really rewinds `moves` moves.
        if self.state.undo_moves(moves) > 0 {
            self.last_move = None;
            self.enter_playing();
        }
    }

    fn handle_click(&mut self, sq: Square) {
        // King placement is a board interaction: only the highlighted pawns
        // are selectable, and clicking one resolves the decision immediately.
        if let Phase::KingPlacement { candidates, .. } = &self.phase {
            if candidates.contains(&sq) {
                let outcome = self.state.place_king(sq);
                self.handle_outcome(outcome);
            }
            return;
        }

        let Phase::Playing { selected, legal } = &self.phase else {
            return;
        };

        // Resolve the click into a move *before* mutating `self`, so the borrow
        // of `self.phase` is done by the time we apply anything.
        let chosen = (*selected).and_then(|sel| {
            // 1. The clicked square is a destination.
            if let Some(mv) = legal.iter().find(|m| m.from == sel && m.to == sq) {
                return Some(*mv);
            }
            // 2. The clicked square holds a piece this move captures, but is not
            //    the destination: a dama jump lands *beyond* its victim and an
            //    en-passant capture lands *in front of* one. The victim is the
            //    piece the player aims at, so accept it when exactly one move
            //    claims it — the landing square works either way.
            let mut claims = legal
                .iter()
                .filter(|m| m.from == sel && m.captured.is_some_and(|(_, victim)| victim == sq));
            match (claims.next(), claims.next()) {
                (Some(mv), None) => Some(*mv),
                _ => None,
            }
        });

        if let Some(mv) = chosen {
            self.play(mv);
            return;
        }

        let own = self.state.board[sq.0][sq.1]
            .map(|p| p.color == self.state.side_to_move())
            .unwrap_or(false);

        let Phase::Playing { selected, .. } = &mut self.phase else {
            return;
        };
        if own {
            *selected = Some(sq);
        } else {
            *selected = None;
        }
    }

    /// Apply `mv` and move the UI to whatever phase the outcome requires.
    fn play(&mut self, mv: Move) {
        let from = mv.from;
        let outcome = self.state.apply(mv);
        self.last_move = Some((from, mv.to));
        self.handle_outcome(outcome);
    }

    fn draw_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Phase::KingPlacement { placing_color, .. } = &self.phase {
                let side = match placing_color {
                    Color::White => "White",
                    Color::Black => "Black",
                };
                ui.label(
                    egui::RichText::new(format!(
                        "Choose a highlighted {side} pawn — it becomes {side}'s King"
                    ))
                    .strong(),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new("Click the pawn on the board")
                        .small()
                        .color(egui::Color32::from_gray(120)),
                );
                return;
            }

            let (side, turn_color) = if self.state.white_to_move {
                ("White", egui::Color32::from_gray(240))
            } else {
                ("Black", egui::Color32::from_gray(80))
            };
            ui.label(egui::RichText::new("●").color(turn_color));
            ui.label(egui::RichText::new(format!("{side} to move")).strong());
            ui.separator();
            if let Some((from, to)) = self.last_move {
                ui.label(format!("last: {from:?} → {to:?}"));
            }
            ui.separator();
            ui.label(
                egui::RichText::new("Z undo  ·  R reset  ·  Esc quit")
                    .small()
                    .color(egui::Color32::from_gray(120)),
            );
        });
    }

    fn draw_board(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let side = available.x.min(available.y);
        let sq = side / 8.0;
        let origin = ui.cursor().min;
        ui.allocate_space(egui::vec2(side, side));

        // Snapshot what we need from the phase *before* the loop, so no borrow
        // of `self.phase` is live when we call `self.handle_click`. Also:
        // filter destinations by the selected square — the Python original did
        // this; my first pass forgot it and highlighted every legal move.
        //
        // Captures are collected from `Move::captured` rather than inferred
        // from "destination square is occupied": that inference only works for
        // chess captures, where the victim *is* the destination. Dama jumps and
        // en passant land on an empty square and leave the victim elsewhere.
        let (selected, legal_targets, capture_targets, king_candidates): (
            Option<Square>,
            Vec<Square>,
            Vec<Square>,
            Vec<Square>,
        ) = match &self.phase {
            Phase::Playing { selected, legal } => {
                let (targets, captures) = selection_highlights(legal, *selected);
                (*selected, targets, captures, Vec::new())
            }
            Phase::KingPlacement { candidates, .. } => {
                (None, Vec::new(), Vec::new(), candidates.clone())
            }
            _ => (None, Vec::new(), Vec::new(), Vec::new()),
        };

        let white_in_check = self.state.is_in_check(Color::White);
        let black_in_check = self.state.is_in_check(Color::Black);

        let dark = egui::Color32::from_rgb(181, 136, 99);
        let light = egui::Color32::from_rgb(240, 217, 181);
        let sel_color = egui::Color32::from_rgba_premultiplied(80, 120, 255, 120);
        let dot_color = egui::Color32::from_rgba_premultiplied(40, 160, 80, 160);
        let last_color = egui::Color32::from_rgba_premultiplied(255, 230, 100, 90);
        let king_sel_color = egui::Color32::from_rgba_premultiplied(255, 200, 40, 120);
        let king_ring_color = egui::Color32::from_rgb(255, 190, 0);
        let check_color = egui::Color32::from_rgba_premultiplied(220, 40, 40, 120);
        let check_ring_color = egui::Color32::from_rgb(230, 30, 30);

        let mut clicked: Option<Square> = None;

        for r in 0..8 {
            for c in 0..8 {
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(c as f32 * sq, r as f32 * sq),
                    egui::vec2(sq, sq),
                );
                let is_king_candidate = king_candidates.contains(&(r, c));

                let bg = if (r + c) % 2 == 0 { light } else { dark };
                ui.painter().rect_filled(rect, 0.0, bg);

                if let Some((from, to)) = self.last_move {
                    if (r, c) == from || (r, c) == to {
                        ui.painter().rect_filled(rect, 0.0, last_color);
                    }
                }

                if Some((r, c)) == selected {
                    ui.painter().rect_filled(rect, 0.0, sel_color);
                }

                // A green ring marks a piece that dies: the destination of a
                // chess capture, the victim a dama jump hops over, or a pawn
                // taken en passant. Quiet destinations get a small green dot.
                // Ringing `capture_targets` is what makes the dama take show
                // up on the jumped piece, which sits between `from` and `to`.
                let is_capture_square = capture_targets.contains(&(r, c));
                if is_capture_square
                    || (legal_targets.contains(&(r, c)) && self.state.board[r][c].is_some())
                {
                    ui.painter().circle_stroke(
                        rect.center(),
                        sq * 0.42,
                        egui::Stroke::new(sq * 0.06, dot_color),
                    );
                } else if legal_targets.contains(&(r, c)) {
                    ui.painter()
                        .circle_filled(rect.center(), sq * 0.14, dot_color);
                }

                if is_king_candidate {
                    ui.painter().rect_filled(rect, 0.0, king_sel_color);
                    ui.painter().circle_stroke(
                        rect.center(),
                        sq * 0.44,
                        egui::Stroke::new(sq * 0.07, king_ring_color),
                    );
                }

                if let Some(p) = self.state.board[r][c] {
                    let king_in_check = p.kind == Kind::King
                        && match p.color {
                            Color::White => white_in_check,
                            Color::Black => black_in_check,
                        };
                    if king_in_check {
                        ui.painter().rect_filled(rect, 0.0, check_color);
                        ui.painter().circle_stroke(
                            rect.center(),
                            sq * 0.44,
                            egui::Stroke::new(sq * 0.07, check_ring_color),
                        );
                    }

                    let glyph = p.kind.unicode();
                    let font = egui::FontId::proportional(sq * 0.78);
                    ui.painter().text(
                        rect.center() + egui::vec2(1.5, 1.5),
                        egui::Align2::CENTER_CENTER,
                        glyph,
                        font.clone(),
                        egui::Color32::from_black_alpha(160),
                    );
                    let fill = match p.color {
                        Color::White => egui::Color32::from_rgb(250, 250, 250),
                        Color::Black => egui::Color32::from_rgb(25, 25, 25),
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        glyph,
                        font,
                        fill,
                    );
                }

                let resp = ui.interact(rect, egui::Id::new(("sq", r, c)), egui::Sense::click());
                // Every square a click acts on gets the hand cursor, including
                // the ringed victim of a dama jump or en passant (its landing
                // square is empty, so the piece itself is the obvious target).
                let actionable =
                    is_king_candidate || is_capture_square || legal_targets.contains(&(r, c));
                let resp = if actionable {
                    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                } else {
                    resp
                };
                if resp.clicked() {
                    clicked = Some((r, c));
                }
            }
        }

        // Deferred click handling — the phase borrow above has ended by now.
        if let Some(sq) = clicked {
            self.handle_click(sq);
        }
    }

    fn draw_modals(&mut self, ctx: &egui::Context) {
        let mut action: Option<Outcome> = None;
        let mut restart = false;
        let mut undo_two = false;

        match &self.phase {
            Phase::Promotion { square, color } => {
                let mut picked: Option<Kind> = None;
                let has_king = self.state.king_of(*color).is_some();
                egui::Window::new("Promotion")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "Promote the {} pawn on {:?}:",
                            if *color == Color::White {
                                "white"
                            } else {
                                "black"
                            },
                            square
                        ));
                        ui.horizontal(|ui| {
                            let choices = [
                                (Kind::Queen, "Queen"),
                                (Kind::Rook, "Rook"),
                                (Kind::Knight, "Knight"),
                            ]
                            .into_iter()
                            .chain((!has_king).then_some((Kind::King, "King")));
                            for (kind, label) in choices {
                                if ui.button(format!("{} {label}", kind.unicode())).clicked() {
                                    picked = Some(kind);
                                }
                            }
                        });
                    });
                if let Some(kind) = picked {
                    action = Some(self.state.choose_promotion(kind));
                }
            }
            Phase::GameOver { message, checkmate } => {
                let title = if *checkmate { "Checkmate" } else { "Stalemate" };
                let can_undo_two = self.state.move_count() >= 2;
                egui::Window::new(title)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.heading(message);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Restart game").clicked() {
                                restart = true;
                            }
                            if ui
                                .add_enabled(can_undo_two, egui::Button::new("Undo 2 moves"))
                                .clicked()
                            {
                                undo_two = true;
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("R restart  ·  Z undo last move")
                                .small()
                                .color(egui::Color32::from_gray(140)),
                        );
                    });
            }
            Phase::Playing { .. } | Phase::KingPlacement { .. } => {}
        }

        if restart {
            self.reset();
            return;
        }
        if undo_two {
            self.undo_moves(2);
            return;
        }

        if let Some(outcome) = action {
            self.handle_outcome(outcome);
        }
    }
}

impl eframe::App for DamaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut reset = false;
        let mut undo = false;
        let mut quit = false;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Z) {
                undo = true;
            }
            if i.key_pressed(egui::Key::R) {
                reset = true;
            }
            if i.key_pressed(egui::Key::Escape) {
                quit = true;
            }
        });
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if reset {
            self.reset();
        }
        if undo {
            self.undo();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.add_space(4.0);
                self.draw_status(ui);
                ui.separator();
                let avail = ui.available_size();
                let side = avail.x.min(avail.y);
                ui.allocate_ui_with_layout(
                    egui::vec2(side, side),
                    egui::Layout::top_down(egui::Align::Min),
                    |child| self.draw_board(child),
                );
            });
        });

        self.draw_modals(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 720.0])
            .with_min_inner_size([480.0, 560.0])
            .with_title("Dama-Chess"),
        ..Default::default()
    };
    eframe::run_native(
        "Dama-Chess",
        options,
        Box::new(|cc| Ok(Box::new(DamaApp::new(cc)))),
    )
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use dama_core::Piece;

    /// A position whose only capture is a dama jump: the white knight on (3,3)
    /// hops over the black rook on (2,2) and lands on the empty (1,1). No chess
    /// move of the knight reaches (2,2), so the victim is only ever visible via
    /// the jump.
    fn dama_jump_state() -> GameState {
        let mut gs = GameState::new();
        gs.board = [[None; 8]; 8];
        gs.white_to_move = true;
        gs.board[3][3] = Some(Piece::new(Color::White, Kind::Knight));
        gs.board[2][2] = Some(Piece::new(Color::Black, Kind::Rook));
        gs
    }

    fn circle_centers(
        shapes: &[egui::Shape],
        predicate: impl Fn(&egui::epaint::CircleShape) -> bool,
    ) -> Vec<egui::Pos2> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Circle(c) if predicate(c) => Some(c.center),
                _ => None,
            })
            .collect()
    }

    /// Repaint the board headlessly and return `(shapes, square_origin, square_size)`.
    fn paint_board(state: GameState, selected: Square) -> (Vec<egui::Shape>, egui::Pos2, f32) {
        let mut app = DamaApp::with_state(state);
        let legal = app.state.legal_moves();
        app.phase = Phase::Playing {
            selected: Some(selected),
            legal,
        };

        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 800.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.draw_board(ui));
        });
        let shapes: Vec<egui::Shape> = output.shapes.into_iter().map(|s| s.shape).collect();

        // The 64 square backgrounds are the only square-on shapes painted, so
        // the panel's own (larger, also square) background is filtered out by
        // taking the smallest square size as the tile size.
        let board: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if (r.rect.width() - r.rect.height()).abs() < 0.01 => {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect();
        let sq = board
            .iter()
            .map(|r| r.width())
            .fold(f32::INFINITY, f32::min);
        let tiles: Vec<&egui::Rect> = board
            .iter()
            .filter(|r| (r.width() - sq).abs() < 0.01)
            .collect();
        assert!(tiles.len() >= 64, "expected at least the 64 board squares");
        let origin = egui::pos2(
            tiles.iter().map(|r| r.min.x).fold(f32::INFINITY, f32::min),
            tiles.iter().map(|r| r.min.y).fold(f32::INFINITY, f32::min),
        );
        (shapes, origin, sq)
    }

    fn center(origin: egui::Pos2, sq: f32, square: Square) -> egui::Pos2 {
        origin + egui::vec2((square.1 as f32 + 0.5) * sq, (square.0 as f32 + 0.5) * sq)
    }

    #[test]
    fn dama_jump_reports_the_hopped_piece_as_the_capture() {
        let gs = dama_jump_state();
        let legal = gs.legal_moves();
        let (targets, captures) = selection_highlights(&legal, Some((3, 3)));

        assert!(targets.contains(&(1, 1)), "jump must land on (1,1)");
        assert_eq!(captures, vec![(2, 2)], "the rook on (2,2) is the victim");
        // Regression guard for the bug this test was written against: the
        // victim is not a destination, so occupancy-based detection missed it.
        assert!(!targets.contains(&(2, 2)));
    }

    #[test]
    fn chess_capture_still_reports_the_destination() {
        let mut gs = dama_jump_state();
        // Knight hop (3,3) -> (1,2) onto a black pawn: victim == destination.
        gs.board[1][2] = Some(Piece::new(Color::Black, Kind::Pawn));
        let legal = gs.legal_moves();
        let (targets, captures) = selection_highlights(&legal, Some((3, 3)));

        assert!(targets.contains(&(1, 2)));
        assert_eq!(captures.len(), 2, "one jump victim, one chess victim");
        assert!(captures.contains(&(2, 2)), "jump over the rook");
        assert!(captures.contains(&(1, 2)), "chess capture of the pawn");
    }

    #[test]
    fn board_paints_a_ring_on_the_jumped_piece_and_a_dot_on_the_landing() {
        let (shapes, origin, sq) = paint_board(dama_jump_state(), (3, 3));
        let victim = center(origin, sq, (2, 2));
        let landing = center(origin, sq, (1, 1));

        let rings = circle_centers(&shapes, |c| {
            c.stroke.width > 0.0 && (c.center - victim).length() < 1.0
        });
        assert_eq!(rings.len(), 1, "the hopped rook must be circled");
        assert!(
            (rings[0] - victim).length() < 1.0,
            "ring must be centred on the jumped piece"
        );

        let dots = circle_centers(&shapes, |c| {
            c.stroke.width == 0.0 && c.radius < sq * 0.3 && (c.center - landing).length() < 1.0
        });
        assert_eq!(dots.len(), 1, "the landing square must show a move dot");
    }

    #[test]
    fn board_paints_a_ring_on_a_chess_capture_destination() {
        let mut gs = dama_jump_state();
        // A plain knight move with a victim standing on the destination:
        // (3,3) -> (1,2) captures a black pawn on (1,2).
        gs.board[1][2] = Some(Piece::new(Color::Black, Kind::Pawn));
        let (shapes, origin, sq) = paint_board(gs, (3, 3));
        let destination = center(origin, sq, (1, 2));

        let rings = circle_centers(&shapes, |c| {
            c.stroke.width > 0.0 && (c.center - destination).length() < 1.0
        });
        assert_eq!(rings.len(), 1, "chess capture destination must be circled");
    }

    /// Make a move the way the UI does: click the piece, then the target.
    fn click_moves(app: &mut DamaApp, from: Square, to: Square) {
        app.handle_click(from);
        app.handle_click(to);
    }

    /// A position where White can take en passant onto the empty (2,2): the
    /// white pawn walks up to row 3, then Black double-pushes beside it.
    fn en_passant_ready() -> DamaApp {
        let mut app = DamaApp::with_state(GameState::new());
        click_moves(&mut app, (6, 1), (4, 1)); // white double push
        click_moves(&mut app, (2, 3), (3, 3)); // black steps out
        click_moves(&mut app, (4, 1), (3, 1)); // white pawn to row 3
        click_moves(&mut app, (1, 2), (3, 2)); // black double push -> ep (2,2)
        assert_eq!(app.state.en_passant, Some((2, 2)));
        app
    }

    fn assert_en_passant_taken(app: &DamaApp) {
        assert!(
            app.state.board[3][2].is_none(),
            "the passed pawn must be removed"
        );
        assert_eq!(
            app.state.board[2][2].map(|p| (p.color, p.kind)),
            Some((Color::White, Kind::Pawn)),
            "the capturer lands on the skipped square"
        );
        assert_eq!(app.state.move_count(), 5);
    }

    #[test]
    fn en_passant_plays_by_clicking_the_landing_square() {
        let mut app = en_passant_ready();
        click_moves(&mut app, (3, 1), (2, 2));
        assert_en_passant_taken(&app);
    }

    #[test]
    fn en_passant_plays_by_clicking_the_captured_pawn() {
        let mut app = en_passant_ready();
        click_moves(&mut app, (3, 1), (3, 2));
        assert_en_passant_taken(&app);
    }

    #[test]
    fn dama_jump_plays_by_clicking_the_jumped_piece() {
        let mut app = DamaApp::with_state(dama_jump_state());
        click_moves(&mut app, (3, 3), (2, 2));
        assert!(app.state.board[2][2].is_none(), "the rook must be removed");
        assert_eq!(
            app.state.board[1][1].map(|p| p.kind),
            Some(Kind::Knight),
            "the jump still lands beyond the victim"
        );
    }
}
