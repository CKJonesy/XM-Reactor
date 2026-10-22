//! The equaliser curve: the one drawn control (see DESIGN.md). A smooth line through a point per
//! band, dragged with the pointer or moved with the arrow keys. A press picks the band nearest the
//! pointer and the drag then sets that band's level; every change goes out as the model's debounced
//! band intent. Until a published view model shows them, the curve keeps the bands it sent as a
//! draft and builds on that, so quick steps are never repeated, dropped or undone by a stale
//! publish.

use super::eq_editor::band_intent;
use super::panel::{Panel, PanelMsg};
use crate::strings;
use crate::view_model::SettingsVm;
use windows_reactor::*;
use xm_core::model::Intent;

/// The flyout's content width (360 less the 12 DIP margins).
const WIDTH: f64 = 336.0;
const PLOT_HEIGHT: f64 = 116.0;
/// Keeps the points at ±band_max inside the plot.
const PAD: f64 = 8.0;
const POINT: f64 = 10.0;
const SELECTED_POINT: f64 = 14.0;
/// Line segments drawn between neighbouring bands.
const STEPS: usize = 8;

#[derive(Debug, Clone, Copy)]
pub enum CurveMsg {
    Press(f64, f64),
    /// y and whether the left button is still down; the band was picked at the press.
    Move(f64, bool),
    /// Release, lost capture or a cancelled pointer.
    End,
    Key(VirtualKey),
    Focus(bool),
}

#[derive(Debug, Default)]
pub struct Curve {
    /// The band the arrow keys move; highlighted while the curve has focus.
    selected: usize,
    /// The band being dragged.
    drag: Option<usize>,
    focused: bool,
    /// The bands last sent, until a publish shows them.
    draft: Option<Vec<i8>>,
}

/// The band nearest `x`; None when there are no bands.
fn band_at(x: f64, n: usize) -> Option<usize> {
    // `as` saturates, so a NaN or negative x lands on the first band.
    (n > 0).then(|| ((x / WIDTH * n as f64).floor() as usize).min(n - 1))
}

/// The whole-dB level at `y`, within ±max.
fn level_at(y: f64, max: i8) -> i8 {
    let max = f64::from(max);
    let t = (y - PAD) / (PLOT_HEIGHT - 2.0 * PAD);
    (max - t * 2.0 * max).round().clamp(-max, max) as i8
}

/// What a press at (x, y), relative to the control, aims at: the band and its new level.
pub fn hit(x: f64, y: f64, n: usize, max: i8) -> Option<(usize, i8)> {
    band_at(x, n).map(|b| (b, level_at(y, max)))
}

/// `pos` counts bands from 0, so band i sits at the middle of the i-th of n equal columns, where
/// its label is.
fn x_of(pos: f64, n: usize) -> f64 {
    (pos + 0.5) * WIDTH / n as f64
}

fn y_of(db: f64, max: i8) -> f64 {
    let max = f64::from(max);
    PAD + (max - db) / (2.0 * max) * (PLOT_HEIGHT - 2.0 * PAD)
}

/// The curve's level at every 1/STEPS of a band, from the first band to the last: a cubic Hermite
/// through each point (Catmull-Rom slopes, one-sided at the ends), clamped to ±max.
pub fn samples(bands: &[i8], max: i8) -> Vec<f64> {
    let y: Vec<f64> = bands.iter().map(|b| f64::from(*b)).collect();
    let n = y.len();
    if n < 2 {
        return y;
    }
    let slope = |i: usize| {
        let (a, b) = (i.saturating_sub(1), (i + 1).min(n - 1));
        (y[b] - y[a]) / (b - a) as f64
    };
    let lim = f64::from(max);
    let mut out = Vec::with_capacity((n - 1) * STEPS + 1);
    for i in 0..n - 1 {
        let (m0, m1) = (slope(i), slope(i + 1));
        for k in 0..STEPS {
            let t = k as f64 / STEPS as f64;
            let (t2, t3) = (t * t, t * t * t);
            let v = (2.0 * t3 - 3.0 * t2 + 1.0) * y[i]
                + (t3 - 2.0 * t2 + t) * m0
                + (-2.0 * t3 + 3.0 * t2) * y[i + 1]
                + (t3 - t2) * m1;
            out.push(v.clamp(-lim, lim));
        }
    }
    out.push(y[n - 1]);
    out
}

impl Curve {
    pub fn update(&mut self, m: CurveMsg, s: &SettingsVm) -> Option<Intent> {
        let mut s = s.clone();
        if let Some(d) = self.draft.as_ref().filter(|d| d.len() == s.bands.len()) {
            s.bands = d.clone();
        }
        let n = s.bands.len();
        let intent = match m {
            CurveMsg::Press(x, y) => {
                let (b, db) = hit(x, y, n, s.band_max).filter(|_| s.bands_enabled)?;
                self.selected = b;
                // A press on the label row only selects.
                if y > PLOT_HEIGHT {
                    return None;
                }
                self.drag = Some(b);
                band_intent(&s, b, f64::from(db))
            }
            CurveMsg::Move(y, true) => {
                let b = self.drag?;
                band_intent(&s, b, f64::from(level_at(y, s.band_max)))
            }
            // A move with the button up or the capture gone means the release went elsewhere.
            CurveMsg::Move(_, false) | CurveMsg::End => {
                self.drag = None;
                None
            }
            CurveMsg::Focus(f) => {
                self.focused = f;
                if !f {
                    self.forget();
                }
                None
            }
            CurveMsg::Key(k) => {
                let b = self.selected.min(n.checked_sub(1)?);
                let step = match k {
                    VirtualKey::LEFT => {
                        self.selected = b.saturating_sub(1);
                        return None;
                    }
                    VirtualKey::RIGHT => {
                        self.selected = (b + 1).min(n - 1);
                        return None;
                    }
                    VirtualKey::UP => 1.0,
                    VirtualKey::DOWN => -1.0,
                    _ => return None,
                };
                self.selected = b;
                band_intent(&s, b, f64::from(s.bands[b]) + step)
            }
        };
        if let Some(Intent::SetEqBands(_, bands)) = &intent {
            self.hold(bands);
        }
        intent
    }

    /// Keeps `bands`, just sent, as the base for the next change and the drawing.
    pub fn hold(&mut self, bands: &[i8]) {
        self.draft = Some(bands.to_vec());
    }

    /// Drops the drag and the draft (another preset, say).
    pub fn forget(&mut self) {
        self.drag = None;
        self.draft = None;
    }

    /// Lays the draft over a freshly published view model; it goes once the publish shows it or
    /// the bands lock.
    pub fn merge(&mut self, s: &mut SettingsVm) {
        match &self.draft {
            Some(d) if s.bands_enabled && d.len() == s.bands.len() && *d != s.bands => {
                s.bands = d.clone();
            }
            _ => self.draft = None,
        }
    }
}

/// What the accessible name adds after the band: why the curve cannot be edited, if it cannot.
fn name_note(s: &SettingsVm) -> &'static str {
    match (s.custom_preset, s.bands_enabled) {
        (false, _) => strings::EQ_BUILT_IN,
        (true, false) => strings::EQ_LOCKED,
        (true, true) => "",
    }
}

/// The arrow keys the curve takes for itself, so the page does not scroll.
pub fn takes_key(k: VirtualKey) -> bool {
    matches!(
        k,
        VirtualKey::LEFT | VirtualKey::RIGHT | VirtualKey::UP | VirtualKey::DOWN
    )
}

pub fn view(s: &SettingsVm, curve: &Curve, cx: &ViewContext<'_, Panel>) -> View {
    let n = s.bands.len();
    let max = s.band_max.max(1);
    let selected = curve.selected.min(n.saturating_sub(1));
    // Custom bands locked by a call or a listening mode, as opposed to a built-in preset's curve.
    let locked = s.custom_preset && !s.bands_enabled;
    let note = name_note(s);
    let mut shapes: Vec<View> = vec![Line::new()
        .x1(0.0)
        .y1(y_of(0.0, max))
        .x2(WIDTH)
        .y2(y_of(0.0, max))
        .stroke(ThemeBrush::PrimaryText)
        .stroke_thickness(1.0)
        .opacity(0.25)
        .into()];
    let pts = samples(&s.bands, max);
    for (k, w) in pts.windows(2).enumerate() {
        let pos = |j: usize| j as f64 / STEPS as f64;
        shapes.push(
            Line::new()
                .x1(x_of(pos(k), n))
                .y1(y_of(w[0], max))
                .x2(x_of(pos(k + 1), n))
                .y2(y_of(w[1], max))
                .stroke(ThemeBrush::Accent)
                .stroke_thickness(2.0)
                .into(),
        );
    }
    for (i, b) in s.bands.iter().enumerate() {
        let lit = curve.focused && i == selected;
        let d = if lit { SELECTED_POINT } else { POINT };
        let mut dot = Ellipse::new()
            .fill(ThemeBrush::Accent)
            .width(d)
            .height(d)
            .canvas_left(x_of(i as f64, n) - d / 2.0)
            .canvas_top(y_of(f64::from(*b), max) - d / 2.0);
        if lit {
            dot = dot.stroke(ThemeBrush::PrimaryText).stroke_thickness(2.0);
        }
        shapes.push(dot.into());
    }
    let labels: Vec<View> = s
        .band_labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            TextBlock::new()
                .text(*l)
                .font_size(11.0)
                .horizontal_alignment(HorizontalAlignment::Center)
                .grid_column(i as i32)
                .into()
        })
        .collect();
    let body = StackPanel::new().spacing(4.0).children((
        Canvas::new()
            .width(WIDTH)
            .height(PLOT_HEIGHT)
            .children(shapes),
        Grid::new()
            .columns(vec![GridLength::STAR; n.max(1)])
            .children(labels),
    ));
    let name = match (s.bands.get(selected), s.band_labels.get(selected)) {
        (Some(g), Some(l)) => strings::curve_name(l, *g, note),
        _ => strings::EQ_CURVE.to_string(),
    };
    let border = Border::new()
        .width(WIDTH)
        .automation_name(name)
        .content(body);
    if locked {
        // The chips' reason line says why the bands are locked.
        return border.is_tab_stop(false).opacity(0.4).into();
    }
    let keys = cx.sender();
    let on_key = RoutedCallback::new(move |k: KeyEventInfo| {
        let take = takes_key(k.key);
        if take {
            _ = keys.send(PanelMsg::Curve(CurveMsg::Key(k.key)));
        }
        take
    });
    // A built-in preset's curve still takes focus and Left/Right, so each band can be read out.
    let border = border
        .is_tab_stop(true)
        .on_got_focus(cx.callback(|_: FocusEventInfo| PanelMsg::Curve(CurveMsg::Focus(true))))
        .on_lost_focus(cx.callback(|_: FocusEventInfo| PanelMsg::Curve(CurveMsg::Focus(false))))
        .on_preview_key_down(on_key);
    if !s.bands_enabled {
        return border.into();
    }
    let border = border
        // A transparent background makes the gaps between the lines take the pointer too.
        .background(Color::transparent())
        .capture_pointer_on_press(true)
        .focus_on_pointer_release(true)
        .on_pointer_pressed(
            cx.callback(|p: PointerEventInfo| PanelMsg::Curve(CurveMsg::Press(p.x, p.y))),
        )
        .on_pointer_released(cx.callback(|_: PointerEventInfo| PanelMsg::Curve(CurveMsg::End)))
        .on_pointer_capture_lost(cx.message(PanelMsg::Curve(CurveMsg::End)))
        .on_pointer_canceled(cx.message(PanelMsg::Curve(CurveMsg::End)));
    // Only while dragging, so hovering never re-renders the panel.
    if curve.drag.is_none() {
        return border.into();
    }
    border
        .on_pointer_moved(cx.callback(|p: PointerEventInfo| {
            PanelMsg::Curve(CurveMsg::Move(
                p.y,
                p.is_left_button_pressed && p.is_captured,
            ))
        }))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(enabled: bool, bands: Vec<i8>) -> SettingsVm {
        SettingsVm {
            bands,
            band_max: 6,
            bands_enabled: enabled,
            ..SettingsVm::default()
        }
    }

    #[test]
    fn pointer_maps_to_the_nearest_band_and_a_whole_db() {
        let col = WIDTH / 10.0;
        // Each band owns its column; the edges and beyond land on the end bands.
        assert_eq!(hit(0.0, 0.0, 10, 6), Some((0, 6)));
        assert_eq!(hit(-20.0, PLOT_HEIGHT + 40.0, 10, 6), Some((0, -6)));
        assert_eq!(hit(col * 0.99, PLOT_HEIGHT / 2.0, 10, 6), Some((0, 0)));
        assert_eq!(hit(col * 1.01, PLOT_HEIGHT / 2.0, 10, 6), Some((1, 0)));
        assert_eq!(hit(WIDTH, PAD, 10, 6), Some((9, 6)));
        assert_eq!(hit(WIDTH + 50.0, PLOT_HEIGHT - PAD, 10, 6), Some((9, -6)));
        assert_eq!(hit(f64::NAN, f64::NAN, 10, 6), Some((0, 0)));
        assert_eq!(hit(10.0, 10.0, 0, 6), None);
        // Snaps to the nearest dB: each dB is (116 - 16) / 12 DIP tall.
        let per_db = (PLOT_HEIGHT - 2.0 * PAD) / 12.0;
        let mid = PLOT_HEIGHT / 2.0;
        assert_eq!(level_at(mid - 2.4 * per_db, 6), 2);
        assert_eq!(level_at(mid - 2.6 * per_db, 6), 3);
        assert_eq!(level_at(mid + 0.4 * per_db, 6), 0);
        // ±10 on Clear Bass models.
        assert_eq!(level_at(PAD, 10), 10);
        // Five bands (older models) at ±10: columns of 67.2 DIP; bottom edge and midpoint.
        assert_eq!(hit(67.0, PLOT_HEIGHT - PAD, 5, 10), Some((0, -10)));
        assert_eq!(hit(68.0, PLOT_HEIGHT / 2.0, 5, 10), Some((1, 0)));
        assert_eq!(hit(WIDTH - 1.0, PLOT_HEIGHT, 5, 10), Some((4, -10)));
        // The drawn point sits where a press would set it.
        for db in -6..=6 {
            assert_eq!(level_at(y_of(f64::from(db), 6), 6), db);
        }
    }

    #[test]
    fn drag_moves_only_the_pressed_band_until_it_ends() {
        let s = vm(true, vec![0; 10]);
        let mut c = Curve::default();
        let top = Some(Intent::SetEqBands(
            None,
            [vec![0; 2], vec![6], vec![0; 7]].concat(),
        ));
        assert_eq!(c.update(CurveMsg::Press(WIDTH * 0.25, 0.0), &s), top);
        // The drag keeps band 2 wherever the pointer goes sideways (moves carry only y).
        let low = c.update(CurveMsg::Move(PLOT_HEIGHT, true), &s);
        assert_eq!(
            low,
            Some(Intent::SetEqBands(
                None,
                [vec![0; 2], vec![-6], vec![0; 7]].concat()
            ))
        );
        // After a release, a capture loss, lost focus or a buttonless move, moves do nothing.
        for end in [
            CurveMsg::End,
            CurveMsg::Focus(false),
            CurveMsg::Move(0.0, false),
        ] {
            c.update(CurveMsg::Press(WIDTH * 0.25, 0.0), &s);
            c.update(end, &s);
            assert_eq!(c.update(CurveMsg::Move(0.0, true), &s), None);
        }
        // Locked bands ignore the pointer.
        let mut c = Curve::default();
        assert_eq!(
            c.update(CurveMsg::Press(10.0, 0.0), &vm(false, vec![0; 10])),
            None
        );
        assert_eq!(c.update(CurveMsg::Move(0.0, true), &s), None);
    }

    #[test]
    fn changes_build_on_the_draft_until_a_publish_shows_it() {
        // Nothing is published between these steps: each builds on the last one sent.
        let s = vm(true, vec![0; 10]);
        let mut c = Curve::default();
        let band = |v: i8| Some(Intent::SetEqBands(None, [vec![v], vec![0; 9]].concat()));
        let at = |db: f64| y_of(db, 6);
        assert_eq!(c.update(CurveMsg::Press(5.0, at(3.0)), &s), band(3));
        // Dragging back to where it started is a change, not "unchanged".
        assert_eq!(c.update(CurveMsg::Move(at(0.0), true), &s), band(0));
        c.update(CurveMsg::End, &s);
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::UP), &s), band(1));
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::UP), &s), band(2));
        // A press on a neighbour keeps band 0 at +2.
        let both = Some(Intent::SetEqBands(None, [vec![2, -4], vec![0; 8]].concat()));
        assert_eq!(c.update(CurveMsg::Press(WIDTH * 0.15, at(-4.0)), &s), both);
        // A stale publish shows the draft; the matching one, or a lock, retires it.
        let mut stale = s.clone();
        c.merge(&mut stale);
        assert_eq!(stale.bands[..2], [2, -4]);
        let mut caught_up = vm(true, [vec![2, -4], vec![0; 8]].concat());
        c.merge(&mut caught_up);
        let mut later = s.clone();
        c.merge(&mut later);
        assert_eq!(later.bands, vec![0; 10]);
        c.hold(&[5; 10]);
        let mut locked = vm(false, vec![0; 10]);
        c.merge(&mut locked);
        assert_eq!(locked.bands, vec![0; 10]);
        // Focus loss and forget drop it too (band 1 is selected since the press).
        let up = Some(Intent::SetEqBands(None, [vec![0, 1], vec![0; 8]].concat()));
        c.hold(&[5; 10]);
        c.update(CurveMsg::Focus(false), &s);
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::UP), &s), up);
        c.forget();
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::UP), &s), up);
    }

    #[test]
    fn name_note_is_one_of_locked_built_in_or_nothing() {
        let note = |custom_preset, bands_enabled| {
            name_note(&SettingsVm {
                custom_preset,
                bands_enabled,
                ..SettingsVm::default()
            })
        };
        assert_eq!(note(true, true), "");
        assert_eq!(note(true, false), strings::EQ_LOCKED);
        // A built-in preset is never "locked", even when its bands are disabled.
        assert_eq!(note(false, false), strings::EQ_BUILT_IN);
        assert_eq!(note(false, true), strings::EQ_BUILT_IN);
    }

    #[test]
    fn a_press_on_the_labels_only_selects() {
        let s = vm(true, vec![0; 10]);
        let mut c = Curve::default();
        assert_eq!(
            c.update(CurveMsg::Press(WIDTH - 1.0, PLOT_HEIGHT + 8.0), &s),
            None
        );
        assert_eq!(c.selected, 9);
        assert_eq!(c.update(CurveMsg::Move(0.0, true), &s), None);
        assert_eq!(
            strings::curve_name("16k", 2, strings::EQ_LOCKED),
            "Equaliser curve, 16 kHz band, +2 dB, locked"
        );
        assert_eq!(
            strings::curve_name("1k", -1, strings::EQ_BUILT_IN),
            "Equaliser curve, 1 kHz band, -1 dB, built-in preset"
        );
    }

    #[test]
    fn arrow_keys_select_and_step_a_band() {
        let mut s = vm(true, vec![0, 6, -6]);
        let mut c = Curve::default();
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::LEFT), &s), None);
        assert_eq!(c.selected, 0);
        assert_eq!(
            c.update(CurveMsg::Key(VirtualKey::UP), &s),
            Some(Intent::SetEqBands(None, vec![1, 6, -6]))
        );
        c.update(CurveMsg::Key(VirtualKey::RIGHT), &s);
        // At the limit nothing changes.
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::UP), &s), None);
        assert_eq!(
            c.update(CurveMsg::Key(VirtualKey::DOWN), &s),
            Some(Intent::SetEqBands(None, vec![1, 5, -6]))
        );
        for _ in 0..5 {
            c.update(CurveMsg::Key(VirtualKey::RIGHT), &s);
        }
        assert_eq!(c.selected, 2);
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::DOWN), &s), None);
        assert!(!takes_key(VirtualKey::TAB) && takes_key(VirtualKey::DOWN));
        // A shorter band list keeps the selection in range.
        s.bands = vec![0];
        assert_eq!(
            c.update(CurveMsg::Key(VirtualKey::UP), &s),
            Some(Intent::SetEqBands(None, vec![1]))
        );
        s.bands_enabled = false;
        assert_eq!(c.update(CurveMsg::Key(VirtualKey::UP), &s), None);
    }

    #[test]
    fn curve_passes_through_every_band_within_the_limit() {
        let bands = [-6, 6, 6, 0, -3, 2, 6, -6, 0, 1];
        let pts = samples(&bands, 6);
        assert_eq!(pts.len(), 9 * STEPS + 1);
        for (i, b) in bands.iter().enumerate() {
            assert_eq!(pts[i * STEPS], f64::from(*b));
        }
        // Just after the second band the spline rises above +6 (about 6.6); the clamp holds it.
        assert!(pts.iter().all(|v| v.abs() <= 6.0));
        assert_eq!(pts[STEPS + 1], 6.0);
        // Flat stays flat, and one band is just its point.
        assert!(samples(&[0; 10], 6).iter().all(|v| *v == 0.0));
        assert_eq!(samples(&[3], 6), vec![3.0]);
    }
}
