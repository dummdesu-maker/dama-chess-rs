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

impl DamaApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let state = GameState::new();
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

        if let Some(sel) = *selected {
            if let Some(mv) = legal.iter().find(|m| m.from == sel && m.to == sq).copied() {
                let from = mv.from;
                let outcome = self.state.apply(mv);
                self.last_move = Some((from, sq));
                self.handle_outcome(outcome);
                return;
            }
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
        let (selected, legal_targets, king_candidates): (Option<Square>, Vec<Square>, Vec<Square>) =
            match &self.phase {
                Phase::Playing { selected, legal } => {
                    let targets = match *selected {
                        Some(f) => legal.iter().filter(|m| m.from == f).map(|m| m.to).collect(),
                        None => Vec::new(),
                    };
                    (*selected, targets, Vec::new())
                }
                Phase::KingPlacement { candidates, .. } => (None, Vec::new(), candidates.clone()),
                _ => (None, Vec::new(), Vec::new()),
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

                if legal_targets.contains(&(r, c)) {
                    if self.state.board[r][c].is_some() {
                        ui.painter().circle_stroke(
                            rect.center(),
                            sq * 0.42,
                            egui::Stroke::new(sq * 0.06, dot_color),
                        );
                    } else {
                        ui.painter()
                            .circle_filled(rect.center(), sq * 0.14, dot_color);
                    }
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
                let resp = if is_king_candidate {
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
