use super::{Param, Sketch};
use crate::midi::MidiState;
use nannou::prelude::*;
use nannou::rand::{thread_rng, Rng};
use std::cell::{Cell, RefCell};

const BOARD_COLS:       usize = 100;
const BOARD_MAX_ROWS:   usize = 300;
const PRESSURE_MAX:     u32   = 3;
const TICK_INTERVAL:    f32   = 1.0;
const AUTO_FILES:       usize = 20;
const CHARGE_PER_TICK:  f32   = 0.20;
const CHARGE_BOOST:     f32   = 0.38;
const CASCADE_INTERVAL: f32   = 0.04;
const CASCADE_BURST:    usize = 4;
const REBUILD_INTERVAL: f32   = 0.04;
const REBUILD_BURST:    usize = 6;
const REBUILD_WAIT:     f32   = 0.7;
const IDLE_THRESHOLD:   u8    = 8;
const CLUSTER_RADIUS:   usize = 3;
const CLUSTER_SIZE:     usize = 4;
const ENERGY_WINDOW:    f32   = 4.0;   // seconds per density half-window
const ENERGY_PEAK:      f32   = 20.0;  // notes/window = full energy bar
const CHAIN_VEL_THRESH: f32   = 0.68;  // velocity threshold for chain ripple

const PARAMS: &[Param] = &[Param::new(2, "expl_sz", 0.5, 2.5)];

// ── Piece model ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Team { White, Black }

fn team_idx(t: Team) -> usize { match t { Team::White => 0, Team::Black => 1 } }

impl Team {
    pub fn opponent(self) -> Team { match self { Team::White => Team::Black, Team::Black => Team::White } }
    pub fn forward(self) -> i32  { match self { Team::White => -1, Team::Black => 1 } }
    pub fn pawn_start_row(self, rows: usize) -> usize { match self { Team::White => rows - 2, Team::Black => 1 } }
    pub fn back_rank(self, rows: usize) -> usize      { match self { Team::White => rows - 1, Team::Black => 0 } }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceKind { Pawn, Knight, Rook, Bishop, Queen, King }

#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub team:   Team,
    pub kind:   PieceKind,
    pub data:   u8,
    pub charge: f32,
    pub lives:  u8,
}

impl Piece {
    fn new(team: Team, kind: PieceKind) -> Self {
        let lives = match kind { PieceKind::Queen => 5, PieceKind::King => 10, _ => 1 };
        Piece { team, kind, data: 0, charge: 0.0, lives }
    }
}

type Board = Vec<Vec<Option<Piece>>>;

fn place(b: &mut Board, team: Team, kind: PieceKind, file: usize, rows: usize) {
    b[team.back_rank(rows)][file] = Some(Piece::new(team, kind));
}

fn fresh_board(cols: usize, rows: usize) -> Board {
    let mut b: Board = vec![vec![None; cols]; BOARD_MAX_ROWS];
    let wp1 = Team::White.pawn_start_row(rows);
    let wp2 = wp1.saturating_sub(1);
    let bp1 = Team::Black.pawn_start_row(rows);
    let bp2 = (bp1 + 1).min(rows.saturating_sub(1));
    for f in 0..cols {
        b[wp1][f] = Some(Piece::new(Team::White, PieceKind::Pawn));
        b[bp1][f] = Some(Piece::new(Team::Black, PieceKind::Pawn));
        if wp2 != Team::White.back_rank(rows) && wp2 != wp1 {
            b[wp2][f] = Some(Piece::new(Team::White, PieceKind::Pawn));
        }
        if bp2 != Team::Black.back_rank(rows) && bp2 != bp1 {
            b[bp2][f] = Some(Piece::new(Team::Black, PieceKind::Pawn));
        }
    }
    for f in (2..cols).step_by(25) {
        place(&mut b, Team::White, PieceKind::Bishop, f, rows);
        place(&mut b, Team::Black, PieceKind::Bishop, f, rows);
    }
    for f in (7..cols).step_by(25) {
        place(&mut b, Team::White, PieceKind::Rook, f, rows);
        place(&mut b, Team::Black, PieceKind::Rook, f, rows);
    }
    for f in (12..cols).step_by(25) {
        place(&mut b, Team::White, PieceKind::Knight, f, rows);
        place(&mut b, Team::Black, PieceKind::Knight, f, rows);
    }
    let queen_f = 18.min(cols - 1);
    let king_f  = 43.min(cols - 1);
    place(&mut b, Team::White, PieceKind::Queen, queen_f, rows);
    place(&mut b, Team::Black, PieceKind::Queen, queen_f, rows);
    let pawn_lives = cols.min(255) as u8;
    for &team in &[Team::White, Team::Black] {
        b[team.back_rank(rows)][king_f] = Some(Piece { team, kind: PieceKind::King, data: 0, charge: 0.0, lives: pawn_lives });
    }
    b
}

// ── Particles ─────────────────────────────────────────────────────────────────

struct Particle { pos: Vec2, vel: Vec2, hue: f32, life: f32, max_life: f32, radius: f32 }
impl Particle {
    fn tick(&mut self, dt: f32) {
        self.pos += self.vel * dt;
        self.vel *= (0.92_f32).powf(dt * 60.0);
        self.life -= dt;
    }
}

pub struct Explosion { particles: Vec<Particle> }
impl Explosion {
    pub fn new(pos: Vec2, victim: Team, scale: f32) -> Self {
        let mut rng = thread_rng();
        let hue: f32 = match victim { Team::White => 0.07, Team::Black => 0.65 };
        let n = (22.0 * scale).round().max(4.0) as usize;
        let particles = (0..n).map(|_| {
            let a = rng.gen_range(0.0..TAU);
            let s = rng.gen_range(80.0..300.0) * scale;
            let l = rng.gen_range(0.35..0.80);
            Particle { pos, vel: vec2(a.cos()*s, a.sin()*s), hue: hue + rng.gen_range(-0.06..0.06),
                       life: l, max_life: l, radius: rng.gen_range(3.0..7.0) * scale }
        }).collect();
        Explosion { particles }
    }
    fn tick(&mut self, dt: f32) { for p in &mut self.particles { p.tick(dt); } self.particles.retain(|p| p.life > 0.0); }
    fn done(&self) -> bool { self.particles.is_empty() }
}

// ── MIDI mapping ──────────────────────────────────────────────────────────────

fn is_black_key(note: u8) -> bool { matches!(note % 12, 1 | 3 | 6 | 8 | 10) }
pub fn note_to_team_file(note: u8, cols: usize) -> (Team, usize) {
    (if is_black_key(note) { Team::Black } else { Team::White }, (note as usize) % cols)
}

// ── Color ─────────────────────────────────────────────────────────────────────

fn hsl_lin(h: f32, s: f32, l: f32, a: f32) -> LinSrgba {
    let c = (1.0 - (2.0*l - 1.0).abs()) * s;
    let h6 = h * 6.0;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r, g, b) = match h6 as u32 {
        0 => (c+m,x+m,m), 1 => (x+m,c+m,m), 2 => (m,c+m,x+m),
        3 => (m,x+m,c+m), 4 => (x+m,m,c+m), _ => (c+m,m,x+m),
    };
    LinSrgba::new(r, g, b, a)
}

// ── Music-reactive types ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum ChordKind { Major, Minor, Diminished, Augmented, Dom7th }

struct ChainRipple {
    file:   usize,
    team:   Team,
    spread: f32,   // half-width in file units, grows over time
    alpha:  f32,
}

struct ShockRing {
    center: Vec2,
    radius: f32,
    max_r:  f32,
    hue:    f32,
    life:   f32,   // 1.0 = fresh, 0.0 = dead
}

struct ResonanceBeam {
    file_a:   usize,
    file_b:   usize,
    life:     f32,
    max_life: f32,
}

// ── Chess sketch ──────────────────────────────────────────────────────────────

pub struct Chess {
    board:          Board,
    cols:           usize,
    game_rows:      Cell<usize>,
    logical_rows:   usize,
    initialized:    Cell<bool>,
    pressure:       Vec<[u32; 2]>,
    explosions:     Vec<Explosion>,
    white_captures: u32,
    black_captures: u32,
    expl_scale:     f32,
    tick_timer:     f32,
    time:           f32,
    cell_heat:      Vec<Vec<f32>>,
    cascade_queue:  Option<Vec<(usize, usize, Team)>>,
    cascade_timer:  f32,
    rebuild_queue:  Option<Vec<(usize, usize, Piece)>>,
    rebuild_timer:  f32,
    rebuild_wait:   f32,
    idle_ticks:     Vec<Vec<u8>>,
    last_occupied:  Vec<Vec<bool>>,
    // Music analysis
    note_timestamps:  Vec<f32>,
    energy:           f32,
    energy_trend:     f32,
    tension:          f32,
    last_pitch_mask:  u16,
    chord_kind:       Option<ChordKind>,
    chord_flash:      f32,
    chord_flash_hue:  f32,
    chain_ripples:    Vec<ChainRipple>,
    shock_rings:      Vec<ShockRing>,
    resonance_beams:  Vec<ResonanceBeam>,
    win:            Cell<Rect>,
    board_tex:      RefCell<Option<wgpu::Texture>>,
    board_tex_win:  Cell<[u32; 2]>,
}

impl Chess {
    pub fn new() -> Self {
        let cols = BOARD_COLS;
        Self {
            board:          vec![vec![None; cols]; BOARD_MAX_ROWS],
            cols,
            game_rows:      Cell::new(0),
            logical_rows:   0,
            initialized:    Cell::new(false),
            pressure:       vec![[0; 2]; cols],
            explosions:     Vec::new(),
            white_captures: 0,
            black_captures: 0,
            expl_scale:     1.0,
            tick_timer:     TICK_INTERVAL,
            time:           0.0,
            cell_heat:      vec![vec![0.0f32; cols]; BOARD_MAX_ROWS],
            cascade_queue:  None,
            cascade_timer:  0.0,
            rebuild_queue:  None,
            rebuild_timer:  0.0,
            rebuild_wait:   0.0,
            idle_ticks:     vec![vec![0u8;    cols]; BOARD_MAX_ROWS],
            last_occupied:  vec![vec![false;  cols]; BOARD_MAX_ROWS],
            note_timestamps:  Vec::new(),
            energy:           0.0,
            energy_trend:     0.0,
            tension:          0.0,
            last_pitch_mask:  0,
            chord_kind:       None,
            chord_flash:      0.0,
            chord_flash_hue:  0.10,
            chain_ripples:    Vec::new(),
            shock_rings:      Vec::new(),
            resonance_beams:  Vec::new(),
            win:            Cell::new(Rect::from_w_h(800.0, 600.0)),
            board_tex:      RefCell::new(None),
            board_tex_win:  Cell::new([0, 0]),
        }
    }

    fn sq(&self) -> f32 { self.win.get().w() / self.cols as f32 }

    pub fn cell_center(&self, file: usize, row: usize) -> Vec2 {
        let r = self.win.get(); let sq = self.sq();
        vec2(r.left() + sq*(file as f32 + 0.5), r.top() - sq*(row as f32 + 0.5))
    }

    fn rows(&self) -> usize { self.logical_rows }

    fn rescale_board(&mut self, old_rows: usize, new_rows: usize) {
        if old_rows == 0 || old_rows == new_rows { return; }
        let mut nb = vec![vec![None; self.cols]; BOARD_MAX_ROWS];
        let mut nh = vec![vec![0.0f32; self.cols]; BOARD_MAX_ROWS];
        for r in 0..old_rows.min(BOARD_MAX_ROWS) {
            for f in 0..self.cols {
                let nr = (r * new_rows / old_rows).min(new_rows.saturating_sub(1));
                if let Some(p) = self.board[r][f] { if nb[nr][f].is_none() { nb[nr][f] = Some(p); } }
                let h = self.cell_heat[r][f];
                if h > 0.0 { nh[nr][f] = nh[nr][f].max(h); }
            }
        }
        self.board = nb; self.cell_heat = nh;
        self.idle_ticks    = vec![vec![0u8;   self.cols]; BOARD_MAX_ROWS];
        self.last_occupied = vec![vec![false;  self.cols]; BOARD_MAX_ROWS];
        self.pressure = vec![[0; 2]; self.cols];
    }

    // ── Music analysis helpers ────────────────────────────────────────────────

    fn pitch_mask(notes: &[crate::midi::NoteState; 128]) -> u16 {
        let mut mask = 0u16;
        for n in 0..128u8 { if notes[n as usize].on { mask |= 1 << (n % 12); } }
        mask
    }

    fn detect_chord(mask: u16) -> Option<ChordKind> {
        let bits: Vec<u8> = (0..12u8).filter(|&i| mask & (1 << i) != 0).collect();
        if bits.len() < 3 { return None; }
        for &root in &bits {
            let ivs: Vec<u8> = bits.iter().map(|&b| (b + 12 - root) % 12).collect();
            if ivs.contains(&4) && ivs.contains(&7) && ivs.contains(&10) { return Some(ChordKind::Dom7th); }
            if ivs.contains(&3) && ivs.contains(&6)                       { return Some(ChordKind::Diminished); }
            if ivs.contains(&4) && ivs.contains(&8)                       { return Some(ChordKind::Augmented); }
            if ivs.contains(&3) && ivs.contains(&7)                       { return Some(ChordKind::Minor); }
            if ivs.contains(&4) && ivs.contains(&7)                       { return Some(ChordKind::Major); }
        }
        None
    }

    // ── Chord special actions ─────────────────────────────────────────────────

    fn execute_chord_action(&mut self, kind: ChordKind) {
        if self.logical_rows == 0 { return; }
        let sq = self.sq();
        let lr = self.logical_rows;
        let cols = self.cols;

        match kind {
            // Harmonic Surge — all ranged pieces charge to full and fire simultaneously
            ChordKind::Major => {
                for r in 0..lr {
                    for f in 0..cols {
                        if let Some(ref mut p) = self.board[r][f] {
                            if matches!(p.kind, PieceKind::Rook | PieceKind::Bishop | PieceKind::Queen) {
                                p.charge = 1.0;
                            }
                        }
                    }
                }
                let mut to_fire: Vec<(usize, usize, Team, PieceKind)> = Vec::new();
                for r in 0..lr {
                    for f in 0..cols {
                        if let Some(p) = self.board[r][f] {
                            if matches!(p.kind, PieceKind::Rook | PieceKind::Bishop | PieceKind::Queen) {
                                to_fire.push((r, f, p.team, p.kind));
                            }
                        }
                    }
                }
                for (r, f, team, pkind) in to_fire {
                    if self.board[r][f].map(|p| p.team == team && p.charge >= 1.0).unwrap_or(false) {
                        let center = self.cell_center(f, r);
                        self.shock_rings.push(ShockRing { center, radius: sq, max_r: sq * 7.0, hue: 0.10, life: 1.0 });
                        match pkind {
                            PieceKind::Rook   => self.fire_rook(team, f, r),
                            PieceKind::Bishop => self.fire_bishop(team, f, r),
                            PieceKind::Queen  => self.fire_queen(team, f, r),
                            _ => {}
                        }
                    }
                }
            }

            // Dark Wave — all knights double-hop forward
            ChordKind::Minor => {
                let mut knights: Vec<(usize, usize, Team)> = Vec::new();
                for r in 0..lr {
                    for f in 0..cols {
                        if let Some(p) = self.board[r][f] {
                            if p.kind == PieceKind::Knight {
                                knights.push((r, f, p.team));
                            }
                        }
                    }
                }
                for &(r, f, _) in &knights {
                    let center = self.cell_center(f, r);
                    self.shock_rings.push(ShockRing { center, radius: sq * 0.5, max_r: sq * 5.5, hue: 0.68, life: 1.0 });
                }
                for &(_, f, team) in &knights {
                    self.try_move(team, f);
                    self.try_move(team, f);
                }
            }

            // Tremor — massive 3-step pawn surge + centered shockwave ring
            ChordKind::Diminished => {
                let mut pawn_files: Vec<(usize, Team)> = Vec::new();
                for f in 0..cols {
                    for &team in &[Team::White, Team::Black] {
                        if self.find_frontmost(team, f)
                            .map(|r| matches!(self.board[r][f], Some(p) if p.kind == PieceKind::Pawn))
                            .unwrap_or(false)
                        {
                            pawn_files.push((f, team));
                        }
                    }
                }
                for _ in 0..3 {
                    for &(f, team) in &pawn_files { self.try_move(team, f); }
                }
                let win = self.win.get();
                self.shock_rings.push(ShockRing {
                    center: win.xy(), radius: sq * 2.0, max_r: win.w() * 0.85, hue: 0.82, life: 1.0,
                });
            }

            // Phase Shift — all bishops charged and fire both diagonals (twice)
            ChordKind::Augmented => {
                for _ in 0..2 {
                    let mut bishops: Vec<(usize, usize, Team)> = Vec::new();
                    for r in 0..lr {
                        for f in 0..cols {
                            if let Some(ref mut p) = self.board[r][f] {
                                if p.kind == PieceKind::Bishop {
                                    p.charge = 1.0;
                                    p.data = 1 - p.data;
                                    bishops.push((r, f, p.team));
                                }
                            }
                        }
                    }
                    for (r, f, team) in bishops {
                        if self.board[r][f].map(|p| p.team == team).unwrap_or(false) {
                            let center = self.cell_center(f, r);
                            self.shock_rings.push(ShockRing { center, radius: sq * 0.5, max_r: sq * 6.0, hue: 0.48, life: 1.0 });
                            self.fire_bishop(team, f, r);
                        }
                    }
                }
            }

            // V7 Discharge — queens fire 3 times rapid-fire + massive central ring
            ChordKind::Dom7th => {
                let win = self.win.get();
                self.shock_rings.push(ShockRing {
                    center: win.xy(), radius: sq * 3.0, max_r: win.w() * 0.75, hue: 0.98, life: 1.0,
                });
                for _ in 0..3 {
                    let mut queens: Vec<(usize, usize, Team)> = Vec::new();
                    for r in 0..lr {
                        for f in 0..cols {
                            if let Some(p) = self.board[r][f] {
                                if p.kind == PieceKind::Queen { queens.push((r, f, p.team)); }
                            }
                        }
                    }
                    for (r, f, team) in queens {
                        if self.board[r][f].map(|p| p.team == team).unwrap_or(false) {
                            if let Some(ref mut p) = self.board[r][f] { p.charge = 1.0; }
                            let center = self.cell_center(f, r);
                            self.shock_rings.push(ShockRing { center, radius: sq, max_r: sq * 9.0, hue: 0.98, life: 1.0 });
                            self.fire_queen(team, f, r);
                        }
                    }
                }
            }
        }
    }

    // ── Movement helpers ──────────────────────────────────────────────────────

    fn find_frontmost(&self, team: Team, file: usize) -> Option<usize> {
        let rows = self.rows();
        match team {
            Team::White => (0..rows).find(|&r| matches!(self.board[r][file], Some(p) if p.team == Team::White)),
            Team::Black => (0..rows).rev().find(|&r| matches!(self.board[r][file], Some(p) if p.team == Team::Black)),
        }
    }

    fn do_capture(&mut self, attacker: Team, cap_file: usize, cap_row: usize, scale: f32) -> bool {
        let victim = match self.board[cap_row][cap_file] { Some(p) => p, None => return false };
        let pos = self.cell_center(cap_file, cap_row);
        match attacker { Team::White => self.white_captures += 1, Team::Black => self.black_captures += 1 }
        if victim.kind == PieceKind::King || victim.lives > 1 {
            self.board[cap_row][cap_file].as_mut().unwrap().lives = victim.lives.saturating_sub(1);
            self.cell_heat[cap_row][cap_file] = 0.65;
            self.explosions.push(Explosion::new(pos, victim.team, scale * 0.5));
            false
        } else {
            self.board[cap_row][cap_file] = None;
            self.cell_heat[cap_row][cap_file] = 1.0;
            self.explosions.push(Explosion::new(pos, victim.team, scale));
            true
        }
    }

    fn add_trail(&mut self, row: usize, file: usize) {
        self.cell_heat[row][file] = self.cell_heat[row][file].max(0.28);
    }

    fn apply_pressure(&mut self, attacker: Team, atk_file: usize, blk_row: usize) -> bool {
        let pidx = team_idx(attacker);
        self.pressure[atk_file][pidx] += 1;
        if self.pressure[atk_file][pidx] >= PRESSURE_MAX {
            self.pressure[atk_file][pidx] = 0;
            self.do_capture(attacker, atk_file, blk_row, self.expl_scale * 2.0)
        } else { false }
    }

    fn try_diagonal_captures(&mut self, team: Team, file: usize, next_row: usize) {
        for &df in &[-1i32, 1i32] {
            let nf = file as i32 + df;
            if nf < 0 || nf as usize >= self.cols { continue; }
            let nf = nf as usize;
            if matches!(self.board[next_row][nf], Some(p) if p.team == team.opponent()) {
                let _ = self.do_capture(team, nf, next_row, self.expl_scale);
            }
        }
    }

    fn promote_pawn(&mut self, team: Team, file: usize, row: usize) {
        let pos = self.cell_center(file, row);
        self.board[row][file] = Some(Piece { team, kind: PieceKind::Queen, data: 0, charge: 0.0, lives: 5 });
        self.cell_heat[row][file] = 1.0;
        self.explosions.push(Explosion::new(pos, team.opponent(), 1.4 * self.expl_scale.max(1.0)));
    }

    // ── Per-piece movement ────────────────────────────────────────────────────

    fn move_pawn(&mut self, team: Team, file: usize, row: usize) {
        let next_r = row as i32 + team.forward();
        if next_r < 0 || next_r as usize >= self.rows() {
            self.promote_pawn(team, file, row);
            return;
        }
        let next_r = next_r as usize;
        self.try_diagonal_captures(team, file, next_r);
        let promote_row = match team { Team::White => 0, Team::Black => self.rows() - 1 };
        match self.board[next_r][file] {
            None => {
                self.add_trail(row, file);
                self.board[next_r][file] = self.board[row][file].take();
                self.pressure[file][team_idx(team)] = 0;
                if next_r == promote_row { self.promote_pawn(team, file, next_r); }
            }
            Some(p) if p.team == team.opponent() => {
                if self.apply_pressure(team, file, next_r) {
                    self.board[next_r][file] = self.board[row][file].take();
                    if next_r == promote_row { self.promote_pawn(team, file, next_r); }
                }
            }
            Some(p) if p.team == team && p.kind == PieceKind::Pawn => {
                let relay_r = next_r as i32 + team.forward();
                if relay_r >= 0 && (relay_r as usize) < self.rows() {
                    let relay_r = relay_r as usize;
                    if self.board[relay_r][file].is_none() {
                        self.add_trail(next_r, file);
                        self.board[relay_r][file] = self.board[next_r][file].take();
                        self.cell_heat[relay_r][file] = self.cell_heat[relay_r][file].max(0.20);
                        if relay_r == promote_row { self.promote_pawn(team, file, relay_r); }
                        self.add_trail(row, file);
                        self.board[next_r][file] = self.board[row][file].take();
                        self.pressure[file][team_idx(team)] = 0;
                        if next_r == promote_row { self.promote_pawn(team, file, next_r); }
                    }
                }
            }
            _ => {}
        }
    }

    fn move_knight(&mut self, team: Team, file: usize, row: usize) {
        let piece = self.board[row][file].unwrap();
        let dest_r = row as i32 + 2 * team.forward();
        if dest_r < 0 || dest_r as usize >= self.rows() { self.board[row][file] = None; return; }
        let dest_r = dest_r as usize;
        let scale = self.expl_scale * 1.2;
        for &df in &[-1i32, 1i32] {
            let dest_f = file as i32 + df;
            if dest_f < 0 || dest_f as usize >= self.cols { continue; }
            let dest_f = dest_f as usize;
            if matches!(self.board[dest_r][dest_f], Some(p) if p.team == team.opponent()) {
                let _ = self.do_capture(team, dest_f, dest_r, scale);
            }
        }
        let pref_df: i32 = if piece.data == 0 { -1 } else { 1 };
        for &df in &[pref_df, -pref_df] {
            let dest_f = file as i32 + df;
            if dest_f < 0 || dest_f as usize >= self.cols { continue; }
            let dest_f = dest_f as usize;
            if self.board[dest_r][dest_f].is_none() {
                self.add_trail(row, file);
                self.board[dest_r][dest_f] = Some(Piece { data: 1 - piece.data, ..piece });
                self.board[row][file] = None;
                return;
            }
        }
    }

    fn fire_rook(&mut self, team: Team, file: usize, row: usize) {
        let fwd = team.forward();
        let mut r = row as i32 + fwd;
        while r >= 0 && (r as usize) < self.rows() {
            let ru = r as usize;
            match self.board[ru][file] {
                Some(p) if p.team == team.opponent() => {
                    let mut tr = row as i32 + fwd;
                    while tr != r { self.cell_heat[tr as usize][file] = self.cell_heat[tr as usize][file].max(0.4); tr += fwd; }
                    let removed = self.do_capture(team, file, ru, self.expl_scale * 1.5);
                    if removed { self.board[ru][file] = self.board[row][file].take(); }
                    if let Some(ref mut p) = self.board[ru][file]  { p.charge = 0.0; }
                    if let Some(ref mut p) = self.board[row][file] { p.charge = 0.0; }
                    return;
                }
                Some(_) => break,
                None => {}
            }
            r += fwd;
        }
        if let Some(ref mut p) = self.board[row][file] { p.charge = 0.55; }
    }

    fn fire_bishop(&mut self, team: Team, file: usize, row: usize) {
        let fwd = team.forward();
        let pref_dx: i32 = if self.board[row][file].map(|p| p.data == 0).unwrap_or(true) { -1 } else { 1 };
        for &dx in &[pref_dx, -pref_dx] {
            let mut r = row as i32 + fwd; let mut f = file as i32 + dx;
            while r >= 0 && (r as usize) < self.rows() && f >= 0 && (f as usize) < self.cols {
                let (ru, fu) = (r as usize, f as usize);
                match self.board[ru][fu] {
                    Some(p) if p.team == team.opponent() => {
                        let new_data = if dx < 0 { 1u8 } else { 0u8 };
                        let removed = self.do_capture(team, fu, ru, self.expl_scale * 1.5);
                        if removed { self.board[ru][fu] = self.board[row][file].take(); }
                        if let Some(ref mut p) = self.board[ru][fu]  { p.charge = 0.0; p.data = new_data; }
                        if let Some(ref mut p) = self.board[row][file] { p.charge = 0.0; p.data = new_data; }
                        return;
                    }
                    Some(_) => break,
                    None => {}
                }
                r += fwd; f += dx;
            }
        }
        if let Some(ref mut p) = self.board[row][file] { p.charge = 0.55; }
    }

    fn fire_queen(&mut self, team: Team, file: usize, row: usize) {
        let dirs: [(i32,i32); 8] = [(-1,0),(1,0),(0,-1),(0,1),(-1,-1),(-1,1),(1,-1),(1,1)];
        let mut best: Option<(usize, usize, i32)> = None;
        for &(dr, df) in &dirs {
            let mut r = row as i32 + dr; let mut f = file as i32 + df; let mut d = 1i32;
            while r >= 0 && (r as usize) < self.rows() && f >= 0 && (f as usize) < self.cols {
                match self.board[r as usize][f as usize] {
                    Some(p) if p.team == team.opponent() => { if best.map(|(_,_,b)| d < b).unwrap_or(true) { best = Some((r as usize, f as usize, d)); } break; }
                    Some(_) => break, None => {}
                }
                r += dr; f += df; d += 1;
            }
        }
        if let Some((tr, tf, _)) = best {
            let removed = self.do_capture(team, tf, tr, self.expl_scale * 1.8);
            if removed && self.board[tr][tf].is_none() { self.board[tr][tf] = self.board[row][file].take(); }
            if let Some(ref mut p) = self.board[tr][tf]  { p.charge = 0.0; }
            if let Some(ref mut p) = self.board[row][file] { p.charge = 0.0; }
        } else {
            if let Some(ref mut p) = self.board[row][file] { p.charge = 0.55; }
        }
    }

    fn move_king(&mut self, team: Team, file: usize, row: usize) {
        let fwd = team.forward();
        let order: [(i32,i32); 8] = [(fwd,0),(fwd,-1),(fwd,1),(0,-1),(0,1),(-fwd,-1),(-fwd,0),(-fwd,1)];
        for &(dr, df) in &order {
            let (nr, nf) = (row as i32 + dr, file as i32 + df);
            if nr < 0 || (nr as usize) >= self.rows() || nf < 0 || (nf as usize) >= self.cols { continue; }
            let (nu, fu) = (nr as usize, nf as usize);
            if matches!(self.board[nu][fu], Some(p) if p.team == team.opponent()) {
                let removed = self.do_capture(team, fu, nu, self.expl_scale * 1.2);
                if removed { self.board[nu][fu] = self.board[row][file].take(); }
                return;
            }
        }
        for &(dr, df) in &order {
            let (nr, nf) = (row as i32 + dr, file as i32 + df);
            if nr < 0 || (nr as usize) >= self.rows() || nf < 0 || (nf as usize) >= self.cols { continue; }
            let (nu, fu) = (nr as usize, nf as usize);
            if self.board[nu][fu].is_none() {
                self.add_trail(row, file);
                self.board[nu][fu] = self.board[row][file].take();
                return;
            }
        }
    }

    fn charge_and_maybe_fire(&mut self, team: Team, file: usize, row: usize, kind: PieceKind) {
        if let Some(ref mut p) = self.board[row][file] { p.charge = (p.charge + CHARGE_BOOST).min(1.0); }
        if self.board[row][file].map(|p| p.charge >= 1.0).unwrap_or(false) {
            match kind {
                PieceKind::Rook   => self.fire_rook(team, file, row),
                PieceKind::Bishop => self.fire_bishop(team, file, row),
                PieceKind::Queen  => self.fire_queen(team, file, row),
                _ => {}
            }
        }
    }

    pub fn try_move(&mut self, team: Team, file: usize) {
        if file >= self.cols || self.rows() == 0 { return; }
        let row = match self.find_frontmost(team, file) { Some(r) => r, None => return };
        let kind = self.board[row][file].unwrap().kind;
        match kind {
            PieceKind::Pawn   => self.move_pawn(team, file, row),
            PieceKind::Knight => self.move_knight(team, file, row),
            PieceKind::Rook   => self.charge_and_maybe_fire(team, file, row, kind),
            PieceKind::Bishop => self.charge_and_maybe_fire(team, file, row, kind),
            PieceKind::Queen  => self.charge_and_maybe_fire(team, file, row, kind),
            PieceKind::King   => self.move_king(team, file, row),
        }
    }

    // ── King death ────────────────────────────────────────────────────────────

    fn count_pawns(&self, team: Team) -> u8 {
        let mut n = 0u8;
        for r in 0..self.logical_rows {
            for f in 0..self.cols {
                if matches!(self.board[r][f], Some(p) if p.team == team && p.kind == PieceKind::Pawn) {
                    n = n.saturating_add(1);
                }
            }
        }
        n
    }

    fn check_king_deaths(&mut self) {
        let mut death: Option<(Team, usize, usize)> = None;
        for &team in &[Team::White, Team::Black] {
            let pawn_count = self.count_pawns(team);
            let mut king_pos: Option<(usize, usize, u8)> = None;
            for r in 0..self.logical_rows {
                for f in 0..self.cols {
                    if let Some(p) = self.board[r][f] {
                        if p.team == team && p.kind == PieceKind::King {
                            king_pos = Some((r, f, p.lives));
                        }
                    }
                }
            }
            if let Some((kr, kf, lives)) = king_pos {
                let new_lives = lives.min(pawn_count);
                if let Some(ref mut p) = self.board[kr][kf] { p.lives = new_lives; }
                if new_lives == 0 { death = Some((team, kr, kf)); break; }
            }
        }
        if let Some((team, kr, kf)) = death {
            self.trigger_king_death(team, kr, kf);
        }
    }

    fn trigger_king_death(&mut self, team: Team, king_row: usize, king_file: usize) {
        let pos = self.cell_center(king_file, king_row);
        self.explosions.push(Explosion::new(pos, team, 5.0 * self.expl_scale.max(1.0)));
        self.cell_heat[king_row][king_file] = 1.0;
        for r in 0..self.logical_rows {
            for f in 0..self.cols {
                if let Some(p) = self.board[r][f] {
                    if p.team == team && p.kind != PieceKind::Pawn { self.board[r][f] = None; }
                }
            }
        }
        let mut pawns: Vec<(usize, usize, Team)> = Vec::new();
        for r in 0..self.logical_rows {
            for f in 0..self.cols {
                if matches!(self.board[r][f], Some(p) if p.team == team && p.kind == PieceKind::Pawn) {
                    pawns.push((r, f, team));
                }
            }
        }
        pawns.sort_by_key(|&(_, f, _)| f);
        self.cascade_queue = Some(pawns);
        self.cascade_timer = 0.0;
    }

    fn start_rebuild(&mut self) {
        for row in &mut self.board { for cell in row { *cell = None; } }
        self.pressure      = vec![[0; 2]; self.cols];
        self.cell_heat     = vec![vec![0.0f32; self.cols]; BOARD_MAX_ROWS];
        self.idle_ticks    = vec![vec![0u8;    self.cols]; BOARD_MAX_ROWS];
        self.last_occupied = vec![vec![false;  self.cols]; BOARD_MAX_ROWS];
        self.white_captures = 0;
        self.black_captures = 0;
        self.note_timestamps.clear();
        self.energy          = 0.0;
        self.energy_trend    = 0.0;
        self.tension         = 0.0;
        self.last_pitch_mask = 0;
        self.chord_flash     = 0.0;
        self.chain_ripples.clear();
        self.shock_rings.clear();
        self.resonance_beams.clear();
        let rows = self.logical_rows;
        let cols = self.cols;
        let new_board = fresh_board(cols, rows);
        let mut pieces: Vec<(usize, usize, Piece)> = Vec::new();
        for r in 0..rows {
            for f in 0..cols {
                if let Some(p) = new_board[r][f] { pieces.push((r, f, p)); }
            }
        }
        let cf = cols / 2;
        pieces.sort_by(|a, b| {
            let ap = (a.2.kind == PieceKind::Pawn) as u8;
            let bp = (b.2.kind == PieceKind::Pawn) as u8;
            if ap != bp { return ap.cmp(&bp); }
            let da = ((a.1 as i32 - cf as i32).abs()) as usize;
            let db = ((b.1 as i32 - cf as i32).abs()) as usize;
            if ap == 1 { db.cmp(&da) } else { da.cmp(&db) }
        });
        self.rebuild_queue = Some(pieces);
        self.rebuild_timer = 0.0;
        self.rebuild_wait  = REBUILD_WAIT;
    }

    fn process_cascade(&mut self, dt: f32) {
        if let Some(mut queue) = self.cascade_queue.take() {
            self.cascade_timer -= dt;
            while self.cascade_timer <= 0.0 {
                for _ in 0..CASCADE_BURST {
                    if queue.is_empty() { break; }
                    let (r, f, team) = queue.remove(0);
                    if self.board[r][f].is_some() {
                        let pos = self.cell_center(f, r);
                        self.board[r][f] = None;
                        self.cell_heat[r][f] = 1.0;
                        self.explosions.push(Explosion::new(pos, team, 2.5 * self.expl_scale.max(1.0)));
                    }
                }
                if queue.is_empty() { break; }
                self.cascade_timer += CASCADE_INTERVAL;
            }
            if queue.is_empty() {
                self.start_rebuild();
            } else {
                self.cascade_queue = Some(queue);
            }
        } else if self.rebuild_queue.is_some() {
            self.rebuild_wait -= dt;
            if self.rebuild_wait <= 0.0 {
                if let Some(mut queue) = self.rebuild_queue.take() {
                    self.rebuild_timer -= dt;
                    while self.rebuild_timer <= 0.0 {
                        for _ in 0..REBUILD_BURST {
                            if queue.is_empty() { break; }
                            let (r, f, p) = queue.remove(0);
                            self.board[r][f] = Some(p);
                            self.cell_heat[r][f] = self.cell_heat[r][f].max(0.55);
                        }
                        if queue.is_empty() { break; }
                        self.rebuild_timer += REBUILD_INTERVAL;
                    }
                    if !queue.is_empty() { self.rebuild_queue = Some(queue); }
                }
            }
        }
    }

    // ── Drawing ───────────────────────────────────────────────────────────────

    fn draw_piece(draw: &Draw, center: Vec2, piece: Piece, sq: f32, time: f32) {
        let r = sq * 0.36;
        let pulse = 0.13 * (time * 3.5 + center.x * 0.04 + center.y * 0.025).sin();
        let gs = 1.9 + pulse;
        let (cx, cy) = (center.x, center.y);

        let diamond = |ax: f32, ay: f32, a: f32, b: f32, angle: f32| -> [Vec2; 4] {
            let raw = [(0.0f32, b), (a, 0.0), (0.0, -b), (-a, 0.0)];
            let (c, s) = (angle.cos(), angle.sin());
            raw.map(|(x, y)| vec2(ax + x*c - y*s, ay + x*s + y*c))
        };

        match piece.kind {
            PieceKind::Pawn => {
                let (hue, fill) = match piece.team {
                    Team::White => (0.08f32, hsl_lin(0.10, 0.15, 0.94, 1.0)),
                    Team::Black => (0.65f32, hsl_lin(0.65, 0.20, 0.07, 1.0)),
                };
                draw.ellipse().xy(center).radius(r * gs).color(hsl_lin(hue, 0.95, 0.58, 0.55 + pulse*0.4));
                draw.ellipse().xy(center).radius(r).color(fill);
            }

            PieceKind::Knight => {
                let (hue, fill) = match piece.team {
                    Team::White => (0.13f32, hsl_lin(0.10, 0.15, 0.94, 1.0)),
                    Team::Black => (0.58f32, hsl_lin(0.65, 0.20, 0.07, 1.0)),
                };
                let glow = hsl_lin(hue, 0.95, 0.58, 0.55 + pulse*0.4);
                let gr = r * (2.1 + pulse);
                draw.quad().points(pt2(cx,cy+gr),pt2(cx+gr,cy),pt2(cx,cy-gr),pt2(cx-gr,cy)).color(glow);
                draw.quad().points(pt2(cx,cy+r), pt2(cx+r, cy),pt2(cx,cy-r), pt2(cx-r, cy)).color(fill);
                let orbit = r * 2.7; let spin = time * 1.1;
                for i in 0..4usize {
                    let angle = spin + (i as f32 / 4.0) * TAU;
                    let (ox, oy) = (cx + orbit*angle.cos(), cy + orbit*angle.sin());
                    let arm = r * 0.22; let w = arm * 0.32; let rot = spin * 0.8;
                    let col = hsl_lin(hue, 0.95, 0.72, 0.82);
                    let [a,b,c,d] = diamond(ox, oy, arm, w, rot);
                    draw.quad().points(a,b,c,d).color(col);
                    let [a,b,c,d] = diamond(ox, oy, w, arm, rot);
                    draw.quad().points(a,b,c,d).color(col);
                }
            }

            PieceKind::Rook => {
                let (hue, fill) = match piece.team {
                    Team::White => (0.05f32, hsl_lin(0.10, 0.15, 0.94, 1.0)),
                    Team::Black => (0.70f32, hsl_lin(0.65, 0.20, 0.07, 1.0)),
                };
                let gr = r * gs;
                draw.rect().xy(center).w_h(gr*2.0, gr*2.0).color(hsl_lin(hue, 0.95, 0.58, 0.50 + pulse*0.3));
                draw.rect().xy(center).w_h(r*2.0, r*2.0).color(fill);
                let n = 8usize; let n_lit = (piece.charge * n as f32).round() as usize;
                let orbit = r * 2.55; let spin = time * 0.55;
                for i in 0..n {
                    let angle = spin + (i as f32 / n as f32) * TAU;
                    let (ox, oy) = (cx + orbit*angle.cos(), cy + orbit*angle.sin());
                    let alive = i < n_lit;
                    let arm = r * (if alive { 0.22 } else { 0.12 });
                    let (alpha, lum) = if alive { (0.90, 0.72) } else { (0.18, 0.40) };
                    let col = hsl_lin(hue, 0.95, lum, alpha);
                    let w = arm * 0.30; let rot = angle;
                    let [a,b,c,d] = diamond(ox, oy, arm, w, rot);
                    draw.quad().points(a,b,c,d).color(col);
                    let [a,b,c,d] = diamond(ox, oy, w, arm, rot);
                    draw.quad().points(a,b,c,d).color(col);
                }
            }

            PieceKind::Bishop => {
                let (hue, fill) = match piece.team {
                    Team::White => (0.14f32, hsl_lin(0.10, 0.15, 0.94, 1.0)),
                    Team::Black => (0.50f32, hsl_lin(0.65, 0.20, 0.07, 1.0)),
                };
                let glow = hsl_lin(hue, 0.95, 0.58, 0.50 + pulse*0.3);
                let gr = r * gs;
                draw.rect().xy(center).w_h(gr*2.2, gr*0.70).color(glow);
                draw.rect().xy(center).w_h(gr*0.70, gr*2.2).color(glow);
                draw.rect().xy(center).w_h(r*2.0, r*0.48).color(fill);
                draw.rect().xy(center).w_h(r*0.48, r*2.0).color(fill);
                let n = 6usize; let n_lit = (piece.charge * n as f32).round() as usize;
                let orbit = r * 2.45; let spin = -time * 0.60;
                for i in 0..n {
                    let angle = spin + (i as f32 / n as f32) * TAU;
                    let (ox, oy) = (cx + orbit*angle.cos(), cy + orbit*angle.sin());
                    let alive = i < n_lit;
                    let dr = r * (if alive { 0.26 } else { 0.13 });
                    let (alpha, lum) = if alive { (0.88, 0.72) } else { (0.18, 0.40) };
                    let [a,b,c,d] = diamond(ox, oy, dr, dr, angle + std::f32::consts::FRAC_PI_4);
                    draw.quad().points(a,b,c,d).color(hsl_lin(hue, 0.95, lum, alpha));
                }
            }

            PieceKind::Queen => {
                let lf = (piece.lives as f32 / 5.0).min(1.0);
                let hue = match piece.team {
                    Team::White => 0.10 - 0.07*(1.0 - lf),
                    Team::Black => 0.62 + 0.08*(1.0 - lf),
                };
                let fill = hsl_lin(hue, 0.90, (0.70*lf).max(0.18), 1.0);
                let glow = hsl_lin(hue, 0.95, 0.60, 0.65 + pulse*0.35);
                let gr = r * (2.5 + pulse*1.3);
                draw.ellipse().xy(center).radius(gr).color(glow);
                let dr = gr * 0.88;
                draw.quad().points(pt2(cx,cy+dr),pt2(cx+dr,cy),pt2(cx,cy-dr),pt2(cx-dr,cy)).color(glow);
                draw.ellipse().xy(center).radius(r * lf.max(0.35)).color(fill);
                if piece.charge > 0.02 {
                    let cp = 0.28 * (time * 8.0).sin() * piece.charge;
                    draw.ellipse().xy(center).radius(r*(2.8 + piece.charge*1.5 + cp))
                        .color(hsl_lin(hue, 0.95, 0.80, piece.charge*0.50));
                }
                let orbit = r * 3.1; let spin = time * 0.38;
                for i in 0..5usize {
                    let angle = spin + (i as f32 / 5.0) * TAU;
                    let (ox, oy) = (cx + orbit*angle.cos(), cy + orbit*angle.sin());
                    let alive = i < piece.lives as usize;
                    let arm = r * (if alive { 0.28 } else { 0.16 });
                    let (alpha, lum) = if alive { (0.95, 0.72) } else { (0.22, 0.35) };
                    let col = hsl_lin(hue, 0.90, lum, alpha);
                    let w = arm * 0.30; let rot = angle + spin * 0.5;
                    let [a,b,c,d] = diamond(ox, oy, arm, w, rot);
                    draw.quad().points(a,b,c,d).color(col);
                    let [a,b,c,d] = diamond(ox, oy, w, arm, rot);
                    draw.quad().points(a,b,c,d).color(col);
                }
            }

            PieceKind::King => {
                let lf = (piece.lives as f32 / BOARD_COLS as f32).min(1.0);
                let hue = match piece.team {
                    Team::White => 0.08 - 0.05 * (1.0 - lf),
                    Team::Black => 0.65 + 0.12 * (1.0 - lf),
                };
                let danger = if lf < 0.2 {
                    ((time * 9.0).sin() * 0.5 + 0.5) * (0.2 - lf) / 0.2
                } else { 0.0 };
                let kr = r * 2.5;
                let spin = time * 0.45;
                let halo_p = (time * 2.1).sin() * 0.12;
                draw.ellipse().xy(center)
                    .radius(kr * (3.5 + halo_p) + danger * kr * 3.0)
                    .color(hsl_lin(hue, 0.90, 0.55, 0.10 + danger * 0.22));
                if danger > 0.01 {
                    draw.ellipse().xy(center)
                        .radius(kr * 2.9 + kr * danger * 2.2)
                        .color(hsl_lin(0.0, 1.0, 0.5, danger * 0.58));
                }
                for i in 0..4 {
                    let angle = spin * 0.5 + (i as f32) * std::f32::consts::FRAC_PI_2;
                    let [p0, p1, p2, p3] = diamond(cx, cy, kr * 0.45, kr * 2.0, angle);
                    draw.quad().points(p0, p1, p2, p3).color(hsl_lin(hue, 0.90, 0.50, 0.72));
                }
                let orbit = kr * 2.85;
                let alive_tiers = ((lf * 10.0).round() as usize).min(10);
                let dot_spin = spin * 0.28 + danger * spin * 0.8;
                for i in 0..10usize {
                    let angle = dot_spin + (i as f32 / 10.0) * TAU - std::f32::consts::FRAC_PI_2;
                    let (ox, oy) = (cx + orbit*angle.cos(), cy + orbit*angle.sin());
                    let alive = i < alive_tiers;
                    let arm = kr * (if alive { 0.23 } else { 0.13 });
                    let (alpha, lum) = if alive { (0.95, 0.70) } else { (0.16, 0.28) };
                    let col = hsl_lin(hue, 0.92, lum, alpha);
                    let w = arm * 0.28; let rot = angle + dot_spin * 1.6;
                    let [a,b,c,d] = diamond(ox, oy, arm, w, rot);
                    draw.quad().points(a,b,c,d).color(col);
                    let [a,b,c,d] = diamond(ox, oy, w, arm, rot);
                    draw.quad().points(a,b,c,d).color(col);
                }
                let fill = hsl_lin(hue, 0.88, 0.55, 1.0);
                draw.ellipse().xy(center).radius(kr).color(fill);
                let cr = kr * 0.72;
                let crown = hsl_lin(0.10, 0.0, 1.0, 0.58 + danger * 0.35);
                draw.rect().xy(center).w_h(cr * 2.3, cr * 0.36).color(crown);
                draw.rect().xy(center).w_h(cr * 0.36, cr * 2.3).color(crown);
                draw.rect().x_y(cx, cy + cr).w_h(cr * 2.5, cr * 0.22).color(crown);
            }
        }
    }

    fn draw_heat(&self, draw: &Draw) {
        let sq = self.sq();
        for row in 0..self.rows().min(BOARD_MAX_ROWS) {
            for file in 0..self.cols {
                let h = self.cell_heat[row][file];
                if h < 0.01 { continue; }
                let center = self.cell_center(file, row);
                draw.rect().xy(center).w_h(sq, sq)
                    .color(hsl_lin(0.05 + 0.08*(1.0-h), 1.0, 0.5, h*0.72));
            }
        }
    }

    fn draw_team_zones(&self, draw: &Draw) {
        if self.logical_rows == 0 { return; }
        let win  = self.win.get();
        let sq   = self.sq();
        let zone = (self.logical_rows / 8).max(1);
        let zh   = sq * zone as f32;
        draw.rect().x_y(win.x(), win.bottom() + zh*0.5).w_h(win.w(), zh)
            .color(hsl_lin(0.08, 0.90, 0.50, 0.09));
        draw.rect().x_y(win.x(), win.top() - zh*0.5).w_h(win.w(), zh)
            .color(hsl_lin(0.62, 0.90, 0.50, 0.09));
    }

    fn draw_tension_overlay(&self, draw: &Draw) {
        if self.tension < 0.12 { return; }
        let win = self.win.get();
        let t = (self.tension - 0.12) / 0.88;
        let pulse = 0.5 + 0.5 * (self.time * (1.5 + t * 7.0)).sin();
        draw.rect().xy(win.xy()).wh(win.wh())
            .color(hsl_lin(0.0, 1.0, 0.42, t * pulse * 0.13));
    }

    fn draw_chord_flash(&self, draw: &Draw) {
        if self.chord_flash < 0.01 { return; }
        let win = self.win.get();
        let t = self.chord_flash / 2.5;
        draw.rect().xy(win.xy()).wh(win.wh())
            .color(hsl_lin(self.chord_flash_hue, 0.95, 0.58, t * t * 0.32));
    }

    fn draw_resonance_beams(&self, draw: &Draw) {
        if self.resonance_beams.is_empty() { return; }
        let win = self.win.get(); let sq = self.sq();
        for beam in &self.resonance_beams {
            let t = beam.life / beam.max_life;
            let x_a = win.left() + sq * (beam.file_a as f32 + 0.5);
            let x_b = win.left() + sq * (beam.file_b as f32 + 0.5);
            let width = (x_b - x_a).abs();
            let mid_x = (x_a + x_b) * 0.5;
            // Soft fill between the two resonant columns
            draw.rect().x_y(mid_x, win.y()).w_h(width, win.h())
                .color(hsl_lin(0.55, 0.90, 0.60, t * 0.12));
            // Bright spine lines at each column
            for &xe in &[x_a, x_b] {
                draw.rect().x_y(xe, win.y()).w_h(sq * 0.22, win.h())
                    .color(hsl_lin(0.55, 0.95, 0.80, t * 0.72));
            }
        }
    }

    fn draw_chain_ripples(&self, draw: &Draw) {
        let win = self.win.get(); let sq = self.sq();
        for rip in &self.chain_ripples {
            let cx = win.left() + sq * (rip.file as f32 + 0.5);
            let hw = (rip.spread * sq).min(win.w() * 0.5);
            let hue = match rip.team { Team::White => 0.09f32, Team::Black => 0.64 };
            // Fading body sweep
            draw.rect().x_y(cx, win.y()).w_h(hw * 2.0, win.h())
                .color(hsl_lin(hue, 0.95, 0.62, rip.alpha * 0.18));
            // Bright leading edges
            for sign in [-1.0f32, 1.0] {
                let ex = cx + sign * hw;
                draw.rect().x_y(ex, win.y()).w_h(sq * 0.9, win.h())
                    .color(hsl_lin(hue, 1.0, 0.82, rip.alpha * 0.72));
            }
        }
    }

    fn draw_shock_rings(&self, draw: &Draw) {
        for ring in &self.shock_rings {
            let t = ring.life;
            let n = 28usize;
            let arm = (ring.radius * 0.085).max(3.0);
            for i in 0..n {
                let angle = (i as f32 / n as f32) * TAU;
                let px = ring.center.x + ring.radius * angle.cos();
                let py = ring.center.y + ring.radius * angle.sin();
                let col = hsl_lin(ring.hue, 0.95, 0.78, t * 0.88);
                let [a, b, c, d] = [
                    pt2(px, py + arm), pt2(px + arm, py),
                    pt2(px, py - arm), pt2(px - arm, py),
                ];
                draw.quad().points(a, b, c, d).color(col);
            }
        }
    }

    fn draw_energy_bar(&self, draw: &Draw) {
        let win = self.win.get(); let sq = self.sq();
        let bar_w = (sq * 2.0).max(12.0);
        let bar_x = win.right() - bar_w * 0.5;

        let trend = self.energy_trend.clamp(-1.0, 1.0);
        // Warm hue when building (trend > 0), cool when fading
        let hue = if trend >= 0.0 {
            0.08 - trend * 0.06
        } else {
            0.62 + (-trend) * 0.18
        };

        // Dark track
        draw.rect().x_y(bar_x, win.y()).w_h(bar_w, win.h())
            .color(hsl_lin(hue, 0.40, 0.06, 0.80));

        // Filled energy level (from bottom)
        let bar_h = (win.h() * self.energy).max(0.0);
        let bar_y = win.bottom() + bar_h * 0.5;
        if bar_h > 1.0 {
            draw.rect().x_y(bar_x, bar_y).w_h(bar_w, bar_h)
                .color(hsl_lin(hue, 0.95, 0.50, 0.85));
        }

        // Pulsing top marker
        let marker_y = win.bottom() + bar_h;
        let pulse = 0.60 + 0.40 * (self.time * (2.5 + self.energy * 6.0)).sin();
        draw.rect().x_y(bar_x, marker_y).w_h(bar_w * 2.5, bar_w * 0.4)
            .color(hsl_lin(hue, 1.0, 0.80, pulse * 0.95));

        // Trend pip: small rect above/below marker indicating direction
        if trend.abs() > 0.05 {
            let pip_y = marker_y + trend.signum() * bar_w * 0.9;
            draw.ellipse().x_y(bar_x, pip_y).radius(bar_w * 0.4)
                .color(hsl_lin(hue, 1.0, 0.88, 0.90));
        }
    }

    fn draw_tension_bars(&self, draw: &Draw) {
        let sq = self.sq();
        for file in 0..self.cols {
            for &team in &[Team::White, Team::Black] {
                let p = self.pressure[file][team_idx(team)];
                if p == 0 { continue; }
                let t = p as f32 / PRESSURE_MAX as f32;
                if let Some(a_row) = self.find_frontmost(team, file) {
                    let b_row = a_row as i32 + team.forward();
                    if b_row < 0 || b_row as usize >= self.rows() { continue; }
                    let b_row = b_row as usize;
                    let ac = self.cell_center(file, a_row);
                    let bc = self.cell_center(file, b_row);
                    let mid = (ac + bc) * 0.5;
                    let h = (bc - ac).length() + sq;
                    draw.rect().xy(mid).w_h(sq*t, h).color(hsl_lin(0.08, 0.95, 0.65, t*0.85));
                }
            }
        }
    }

    fn draw_explosions(&self, draw: &Draw) {
        for expl in &self.explosions {
            for p in &expl.particles {
                let t = (p.life / p.max_life).max(0.0);
                draw.ellipse().xy(p.pos).radius(p.radius * t)
                    .color(hsl_lin(p.hue, 0.88, 0.55 + 0.25*(1.0-t), t*t));
            }
        }
    }
}

impl Sketch for Chess {
    fn update(&mut self, midi: &MidiState, dt: f32) {
        self.time += dt;

        let cur_rows = self.game_rows.get();
        if !self.initialized.get() && cur_rows > 0 {
            self.board = fresh_board(self.cols, cur_rows);
            self.logical_rows = cur_rows;
            self.initialized.set(true);
        } else if self.initialized.get() && cur_rows != self.logical_rows && cur_rows > 0 {
            self.rescale_board(self.logical_rows, cur_rows);
            self.logical_rows = cur_rows;
        }

        self.expl_scale = PARAMS[0].read(midi);

        if self.initialized.get() {
            // ── Always: energy tracking (runs even during cascade) ────────
            for ev in midi.note_on_events() {
                self.note_timestamps.push(self.time);
                let _ = ev; // timestamps used; moves handled below during normal play
            }
            self.note_timestamps.retain(|&t| self.time - t < ENERGY_WINDOW * 2.0);
            let recent = self.note_timestamps.iter().filter(|&&t| self.time - t < ENERGY_WINDOW).count();
            let older  = self.note_timestamps.iter().filter(|&&t| {
                let age = self.time - t; age >= ENERGY_WINDOW && age < ENERGY_WINDOW * 2.0
            }).count();
            let raw_energy = (recent as f32 / ENERGY_PEAK).min(1.0);
            self.energy       = self.energy * 0.92 + raw_energy * 0.08;
            self.energy_trend = self.energy_trend * 0.90
                + ((recent as f32 - older as f32) / ENERGY_PEAK) * 0.10;

            // Tension from currently held notes
            {
                let held: Vec<u8> = (0..128u8).filter(|&n| midi.notes[n as usize].on).collect();
                let raw_tension = if held.len() < 2 { 0.0f32 } else {
                    let mut min_cons = 1.0f32;
                    for i in 0..held.len() {
                        for j in (i+1)..held.len() {
                            let s = (held[j] as i32 - held[i] as i32).abs() as u8 % 12;
                            let c: f32 = match s {
                                0 => 1.00, 7 => 0.85, 5 => 0.80, 4 => 0.70,
                                3 => 0.65, 9 => 0.60, 8 => 0.55, 2 => 0.35,
                                10 => 0.30, 11 => 0.15, 1 => 0.10, 6 => 0.00, _ => 0.50,
                            };
                            if c < min_cons { min_cons = c; }
                        }
                    }
                    1.0 - min_cons
                };
                self.tension = self.tension * 0.88 + raw_tension * 0.12;
            }

            // ── Always: visual effect decay ───────────────────────────────
            for ring in &mut self.shock_rings {
                ring.radius += (ring.max_r - ring.radius) * dt * 3.5;
                ring.life = (ring.life - dt * 0.55).max(0.0);
            }
            self.shock_rings.retain(|r| r.life > 0.01);
            for rip in &mut self.chain_ripples {
                rip.spread += dt * 35.0;
                rip.alpha = (rip.alpha - dt * 1.1).max(0.0);
            }
            self.chain_ripples.retain(|r| r.alpha > 0.01);
            for beam in &mut self.resonance_beams { beam.life -= dt; }
            self.resonance_beams.retain(|b| b.life > 0.0);
            self.chord_flash = (self.chord_flash - dt * 0.65).max(0.0);

            if self.cascade_queue.is_some() || self.rebuild_queue.is_some() {
                // Keep mask current so no stale chord fires when play resumes
                self.last_pitch_mask = Self::pitch_mask(&midi.notes);
                self.process_cascade(dt);
            } else {
                // ── Normal play ───────────────────────────────────────────

                for ev in midi.note_on_events() {
                    let (team, file) = note_to_team_file(ev.note, self.cols);

                    // Perfect 5th resonance: when new note forms a P5 with a held note,
                    // both files activate simultaneously and a beam connects them.
                    'fifth: for n in 0..128u8 {
                        if midi.notes[n as usize].on && n != ev.note {
                            let semi = (ev.note as i32 - n as i32).abs() as u8 % 12;
                            if semi == 7 {
                                let (ot, of_) = note_to_team_file(n, self.cols);
                                self.try_move(ot, of_);
                                if of_ != file {
                                    let (fa, fb) = if file < of_ { (file, of_) } else { (of_, file) };
                                    if !self.resonance_beams.iter().any(|b| b.file_a == fa && b.file_b == fb) {
                                        self.resonance_beams.push(ResonanceBeam {
                                            file_a: fa, file_b: fb, life: 2.0, max_life: 2.0,
                                        });
                                    }
                                }
                                break 'fifth;
                            }
                        }
                    }

                    self.try_move(team, file);

                    // High velocity: chain ripple to ±1 and ±2 adjacent files
                    if ev.velocity >= CHAIN_VEL_THRESH {
                        for df in [-2i32, -1, 1, 2] {
                            let nf = file as i32 + df;
                            if nf >= 0 && nf < self.cols as i32 {
                                self.try_move(team, nf as usize);
                            }
                        }
                        self.chain_ripples.push(ChainRipple { file, team, spread: 0.5, alpha: 1.0 });
                    }
                }

                // Chord detection: fires a special action when the held chord changes
                let cur_mask = Self::pitch_mask(&midi.notes);
                if cur_mask != self.last_pitch_mask {
                    self.last_pitch_mask = cur_mask;
                    if let Some(ckind) = Self::detect_chord(cur_mask) {
                        self.chord_kind = Some(ckind);
                        self.chord_flash_hue = match ckind {
                            ChordKind::Major      => 0.10,
                            ChordKind::Minor      => 0.68,
                            ChordKind::Diminished => 0.82,
                            ChordKind::Augmented  => 0.48,
                            ChordKind::Dom7th     => 0.98,
                        };
                        self.chord_flash = 2.5;
                        self.execute_chord_action(ckind);
                    }
                }

                // ── Auto-tick ─────────────────────────────────────────────
                self.tick_timer -= dt;
                if self.tick_timer <= 0.0 {
                    self.tick_timer = TICK_INTERVAL;

                    // Passive charge for ranged pieces
                    for r in 0..self.logical_rows {
                        for f in 0..self.cols {
                            if let Some(ref mut p) = self.board[r][f] {
                                if matches!(p.kind, PieceKind::Rook | PieceKind::Bishop | PieceKind::Queen) {
                                    p.charge = (p.charge + CHARGE_PER_TICK).min(1.0);
                                }
                            }
                        }
                    }

                    // Fire fully charged pieces
                    let mut charged: Vec<(usize, usize, Team, PieceKind)> = Vec::new();
                    for r in 0..self.logical_rows {
                        for f in 0..self.cols {
                            if let Some(p) = self.board[r][f] {
                                if p.charge >= 1.0 { charged.push((r, f, p.team, p.kind)); }
                            }
                        }
                    }
                    for (r, f, team, kind) in charged {
                        if self.board[r][f].map(|p| p.team == team && p.charge >= 1.0).unwrap_or(false) {
                            match kind {
                                PieceKind::Rook   => self.fire_rook(team, f, r),
                                PieceKind::Bishop => self.fire_bishop(team, f, r),
                                PieceKind::Queen  => self.fire_queen(team, f, r),
                                _ => {}
                            }
                        }
                    }

                    // Auto-advance non-ranged pieces
                    let mut rng = thread_rng();
                    for _ in 0..AUTO_FILES {
                        let f = rng.gen_range(0..self.cols);
                        for &team in &[Team::White, Team::Black] {
                            let row_opt = match team {
                                Team::White => (0..self.logical_rows).find(|&r| matches!(self.board[r][f], Some(p) if p.team == Team::White)),
                                Team::Black => (0..self.logical_rows).rev().find(|&r| matches!(self.board[r][f], Some(p) if p.team == Team::Black)),
                            };
                            if let Some(row) = row_opt {
                                if let Some(p) = self.board[row][f] {
                                    if matches!(p.kind, PieceKind::Pawn | PieceKind::Knight | PieceKind::King) {
                                        self.try_move(team, f);
                                    }
                                }
                            }
                        }
                    }

                    // Idle tracking
                    for r in 0..self.logical_rows {
                        for f in 0..self.cols {
                            let occ = self.board[r][f].is_some();
                            if occ && self.last_occupied[r][f] {
                                self.idle_ticks[r][f] = self.idle_ticks[r][f].saturating_add(1);
                            } else {
                                self.idle_ticks[r][f] = 0;
                            }
                            self.last_occupied[r][f] = occ;
                        }
                    }

                    // Force-move idle pieces
                    let mut idle_moves: Vec<(usize, usize, Team, PieceKind)> = Vec::new();
                    for r in 0..self.logical_rows {
                        for f in 0..self.cols {
                            if self.idle_ticks[r][f] >= IDLE_THRESHOLD {
                                if let Some(p) = self.board[r][f] {
                                    idle_moves.push((r, f, p.team, p.kind));
                                }
                            }
                        }
                    }
                    for (r, f, team, kind) in idle_moves {
                        if self.board[r][f].map(|p| p.team == team).unwrap_or(false) {
                            if matches!(kind, PieceKind::Rook | PieceKind::Bishop | PieceKind::Queen) {
                                if let Some(ref mut p) = self.board[r][f] {
                                    p.charge = (p.charge + 0.35).min(1.0);
                                }
                            } else {
                                self.try_move(team, f);
                                self.idle_ticks[r][f] = 0;
                            }
                        }
                    }

                    // Pawn cluster rush
                    for &team in &[Team::White, Team::Black] {
                        let pawn_files: Vec<usize> = (0..self.cols).filter(|&f| {
                            self.find_frontmost(team, f)
                                .map(|r| matches!(self.board[r][f], Some(p) if p.kind == PieceKind::Pawn))
                                .unwrap_or(false)
                        }).collect();

                        let cluster_files: Vec<usize> = pawn_files.iter().copied().filter(|&f| {
                            pawn_files.iter().filter(|&&g| {
                                (g as i32 - f as i32).unsigned_abs() as usize <= CLUSTER_RADIUS
                            }).count() >= CLUSTER_SIZE
                        }).collect();

                        if !cluster_files.is_empty() {
                            let steps = rng.gen_range(1usize..=4usize);
                            for _ in 0..steps {
                                for &f in &cluster_files { self.try_move(team, f); }
                            }
                        }
                    }
                }

                self.check_king_deaths();
            }
        }

        let decay = (1.0 - dt * 1.5).max(0.0);
        for row in &mut self.cell_heat { for h in row { *h *= decay; } }
        for e in &mut self.explosions { e.tick(dt); }
        self.explosions.retain(|e| !e.done());
    }

    fn view(&self, draw: &Draw, win: Rect) {
        self.win.set(win);
        let sq   = self.sq();
        let rows = ((win.h() / sq).ceil() as usize).min(BOARD_MAX_ROWS);
        self.game_rows.set(rows);

        if let Some(ref tex) = *self.board_tex.borrow() {
            draw.texture(tex).xy(win.xy()).wh(win.wh());
        }
        self.draw_team_zones(draw);
        self.draw_resonance_beams(draw);
        self.draw_heat(draw);
        self.draw_tension_bars(draw);
        self.draw_tension_overlay(draw);
        self.draw_chord_flash(draw);
        self.draw_chain_ripples(draw);

        let sq   = self.sq();
        let time = self.time;
        for row in 0..self.rows() {
            for file in 0..self.cols {
                if let Some(piece) = self.board[row][file] {
                    Self::draw_piece(draw, self.cell_center(file, row), piece, sq, time);
                }
            }
        }

        self.draw_shock_rings(draw);
        self.draw_explosions(draw);
        self.draw_energy_bar(draw);
    }

    fn raw_render(&self, device: &wgpu::Device, queue: &wgpu::Queue,
                  _encoder: &mut wgpu::CommandEncoder, _target: &wgpu::TextureViewHandle, _win: Rect) {
        let win = self.win.get();
        let win_w = win.w() as u32; let win_h = win.h() as u32;
        if win_w == 0 || win_h == 0 { return; }
        let cur = self.board_tex_win.get();
        if cur[0] == win_w && cur[1] == win_h { return; }
        let sq_px = (win.w() / self.cols as f32).max(1.0) as u32;
        let mut pixels = vec![0u8; (win_w * win_h * 4) as usize];
        for py in 0..win_h {
            for px in 0..win_w {
                let light = ((px / sq_px) + (py / sq_px)) % 2 == 0;
                let v: u8 = if light { 209 } else { 26 };
                let i = ((py * win_w + px) * 4) as usize;
                pixels[i] = v; pixels[i+1] = v; pixels[i+2] = v; pixels[i+3] = 255;
            }
        }
        let tex = wgpu::TextureBuilder::new().size([win_w, win_h])
            .format(wgpu::TextureFormat::Rgba8Unorm)
            .usage(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST)
            .build(device);
        queue.write_texture(
            wgpu::ImageCopyTexture { texture: &*tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &pixels,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(win_w * 4), rows_per_image: None },
            wgpu::Extent3d { width: win_w, height: win_h, depth_or_array_layers: 1 },
        );
        *self.board_tex.borrow_mut() = Some(tex);
        self.board_tex_win.set([win_w, win_h]);
    }

    fn name(&self) -> &'static str { "chess" }
    fn params(&self) -> &[Param] { PARAMS }

    fn hud_info(&self) -> Option<String> {
        let mut q_w = "-".to_string(); let mut q_b = "-".to_string();
        let mut k_w = "-".to_string(); let mut k_b = "-".to_string();
        let mut pw = 0u32; let mut pb = 0u32;
        for r in 0..self.logical_rows {
            for f in 0..self.cols {
                if let Some(p) = self.board[r][f] {
                    match (p.team, p.kind) {
                        (Team::White, PieceKind::Queen) => q_w = p.lives.to_string(),
                        (Team::Black, PieceKind::Queen) => q_b = p.lives.to_string(),
                        (Team::White, PieceKind::King)  => k_w = p.lives.to_string(),
                        (Team::Black, PieceKind::King)  => k_b = p.lives.to_string(),
                        (Team::White, PieceKind::Pawn)  => pw += 1,
                        (Team::Black, PieceKind::Pawn)  => pb += 1,
                        _ => {}
                    }
                }
            }
        }
        let chord_str = match self.chord_kind {
            Some(ChordKind::Major)      => " | MAJOR surge",
            Some(ChordKind::Minor)      => " | MINOR wave",
            Some(ChordKind::Diminished) => " | DIM tremor",
            Some(ChordKind::Augmented)  => " | AUG shift",
            Some(ChordKind::Dom7th)     => " | V7 discharge",
            None => "",
        };
        let e_filled = (self.energy * 8.0).round() as usize;
        let e_bar: String = "#".repeat(e_filled) + &".".repeat(8 - e_filled.min(8));
        let trend_ch = if self.energy_trend > 0.04 { "^" } else if self.energy_trend < -0.04 { "v" } else { "-" };
        Some(format!(
            "pawns W:{pw} B:{pb}  |  queen W:{q_w} B:{q_b}  |  king W:{k_w} B:{k_b}  |  caps W:{} B:{}  |  [{}]{}{chord_str}",
            self.white_captures, self.black_captures, e_bar, trend_ch
        ))
    }

    fn actions(&self) -> &[(Key, &'static str)] {
        &[(Key::R, "Reset board")]
    }

    fn key_pressed(&mut self, key: Key) {
        if key == Key::R {
            self.cascade_queue = None;
            self.rebuild_queue = None;
            let rows = self.logical_rows;
            if rows > 0 { self.board = fresh_board(self.cols, rows); }
            self.pressure      = vec![[0; 2]; self.cols];
            self.cell_heat     = vec![vec![0.0f32; self.cols]; BOARD_MAX_ROWS];
            self.idle_ticks    = vec![vec![0u8;    self.cols]; BOARD_MAX_ROWS];
            self.last_occupied = vec![vec![false;  self.cols]; BOARD_MAX_ROWS];
            self.explosions.clear();
            self.white_captures = 0;
            self.black_captures = 0;
            self.note_timestamps.clear();
            self.energy          = 0.0;
            self.energy_trend    = 0.0;
            self.tension         = 0.0;
            self.last_pitch_mask = 0;
            self.chord_flash     = 0.0;
            self.chain_ripples.clear();
            self.shock_rings.clear();
            self.resonance_beams.clear();
        }
    }
}
