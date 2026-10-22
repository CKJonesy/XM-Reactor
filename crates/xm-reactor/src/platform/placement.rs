#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarEdge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Screen rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Where the flyout goes: 12 DIP in from the taskbar edge. On a bottom or top taskbar it sits in
/// the tray corner like Windows' own flyouts; on a side taskbar it is centred on the tray icon (or
/// in the corner when the icon is unknown or on another monitor). Kept fully inside `work_area`.
pub fn place(
    work_area: Rect,
    monitor: Rect,
    edge: TaskbarEdge,
    icon: Option<Rect>,
    size_dip: (f64, f64),
    dpi_scale: f64,
) -> Rect {
    let px = |dip: f64| (dip * dpi_scale).round() as i32;
    let (w, h, gap) = (px(size_dip.0), px(size_dip.1), px(12.0));
    let icon_centre = icon
        .map(|i| {
            (
                (i.left + i.right).div_euclid(2),
                (i.top + i.bottom).div_euclid(2),
            )
        })
        .filter(|&(x, y)| {
            x >= monitor.left && x < monitor.right && y >= monitor.top && y < monitor.bottom
        });
    let (x, y) = match edge {
        TaskbarEdge::Bottom | TaskbarEdge::Top => (
            work_area.right - gap - w,
            if edge == TaskbarEdge::Bottom {
                work_area.bottom - gap - h
            } else {
                work_area.top + gap
            },
        ),
        TaskbarEdge::Left | TaskbarEdge::Right => (
            if edge == TaskbarEdge::Right {
                work_area.right - gap - w
            } else {
                work_area.left + gap
            },
            icon_centre.map_or(work_area.bottom - gap - h, |(_, cy)| cy - h / 2),
        ),
    };
    // Keep the gap from every work-area edge; when the flyout cannot fit, pin it to the start.
    let fit = |v: i32, lo: i32, hi: i32, len: i32| {
        if hi - lo < len + 2 * gap {
            lo
        } else {
            v.clamp(lo + gap, hi - gap - len)
        }
    };
    let x = fit(x, work_area.left, work_area.right, w);
    let y = fit(y, work_area.top, work_area.bottom, h);
    Rect {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

/// The tallest the flyout may be, in DIP: `height_dip`, or the work area less the 12 DIP gap at
/// each end when that is shorter, so the footer is never off screen.
pub fn fit_height(height_dip: f64, work_area: Rect, dpi_scale: f64) -> f64 {
    let room = f64::from(work_area.bottom - work_area.top) / dpi_scale - 24.0;
    height_dip.min(room)
}

/// The open flyout at a new `height` (px): same left and width, the edge nearest the taskbar kept
/// (the top for a top taskbar, otherwise the bottom), and kept inside `work_area`.
pub fn resized(current: Rect, height: i32, work_area: Rect, keep_top: bool) -> Rect {
    let height = height.min(work_area.bottom - work_area.top);
    let top = if keep_top {
        current.top.min(work_area.bottom - height)
    } else {
        (current.bottom - height).max(work_area.top)
    };
    Rect {
        top,
        bottom: top + height,
        ..current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    const SIZE: (f64, f64) = (360.0, 600.0);

    #[test]
    fn placement_bottom_right_icon() {
        // 1920x1080 at 100 %, 48 px taskbar at the bottom, icon near the right end.
        let mon = r(0, 0, 1920, 1080);
        let work = r(0, 0, 1920, 1032);
        let icon = r(1700, 1040, 1724, 1064);
        let p = place(work, mon, TaskbarEdge::Bottom, Some(icon), SIZE, 1.0);
        // In the tray corner, whatever the icon's position.
        assert_eq!(p, r(1548, 420, 1908, 1020));
    }

    #[test]
    fn placement_top_edge() {
        let mon = r(0, 0, 1920, 1080);
        let work = r(0, 48, 1920, 1080);
        let icon = r(1500, 12, 1524, 36);
        let p = place(work, mon, TaskbarEdge::Top, Some(icon), SIZE, 1.0);
        assert_eq!(p, r(1548, 60, 1908, 660));
    }

    #[test]
    fn placement_left_edge() {
        let mon = r(0, 0, 1920, 1080);
        let work = r(60, 0, 1920, 1080);
        let icon = r(18, 900, 42, 924);
        let p = place(work, mon, TaskbarEdge::Left, Some(icon), SIZE, 1.0);
        // Centred on the icon vertically would run off the bottom, so it is clamped.
        assert_eq!(p, r(72, 468, 432, 1068));
    }

    #[test]
    fn placement_right_edge() {
        let mon = r(0, 0, 1920, 1080);
        let work = r(0, 0, 1860, 1080);
        let icon = r(1878, 400, 1902, 424);
        let p = place(work, mon, TaskbarEdge::Right, Some(icon), SIZE, 1.0);
        assert_eq!(p, r(1488, 112, 1848, 712));
    }

    #[test]
    fn placement_second_monitor_150pct() {
        // Second monitor to the right of a 1920 px primary, 2560x1440 at 150 %, 72 px taskbar.
        let mon = r(1920, 0, 4480, 1440);
        let work = r(1920, 0, 4480, 1368);
        let icon = r(4000, 1386, 4036, 1422);
        let p = place(work, mon, TaskbarEdge::Bottom, Some(icon), SIZE, 1.5);
        // 540 x 900 px, 18 px gap from the corner.
        assert_eq!(p, r(3922, 450, 4462, 1350));
    }

    #[test]
    fn tall_flyout_fits_1080p_at_150pct() {
        // 1920 x 1080 at 150 %: a 72 px taskbar leaves a 1008 px work area (672 DIP).
        let mon = r(0, 0, 1920, 1080);
        let work = r(0, 0, 1920, 1008);
        let h = fit_height(770.0, work, 1.5);
        assert_eq!(h, 648.0);
        assert_eq!(fit_height(600.0, work, 1.5), 600.0);
        let p = place(work, mon, TaskbarEdge::Bottom, None, (360.0, h), 1.5);
        // 540 x 972 px with the 18 px gap at both ends: the footer stays on screen.
        assert_eq!(p, r(1362, 18, 1902, 990));
    }

    #[test]
    fn resize_keeps_left_and_the_taskbar_side() {
        let work = r(0, 0, 1920, 1032);
        // 730 to 960 and back with a bottom taskbar: the bottom edge stays put.
        let short = r(1000, 290, 1360, 1020);
        let tall = resized(short, 960, work, false);
        assert_eq!(tall, r(1000, 60, 1360, 1020));
        assert_eq!(resized(tall, 730, work, false), short);
        // A top taskbar keeps the top edge.
        assert_eq!(
            resized(r(1000, 12, 1360, 742), 960, work, true),
            r(1000, 12, 1360, 972)
        );
        // Never above or below the work area.
        assert_eq!(resized(short, 1200, work, false), r(1000, 0, 1360, 1032));
        assert_eq!(
            resized(r(1000, 900, 1360, 1020), 400, work, true),
            r(1000, 632, 1360, 1032)
        );
    }

    #[test]
    fn placement_negative_coordinates() {
        // Monitor left of the primary: same offsets as `placement_bottom_right_icon`, shifted by -2560.
        let mon = r(-2560, 0, 0, 1440);
        let work = r(-2560, 0, 0, 1392);
        let icon = r(-313, 1400, -288, 1424); // centre -300.5 rounds down to -301
        let p = place(work, mon, TaskbarEdge::Bottom, Some(icon), SIZE, 1.0);
        assert_eq!(p, r(-372, 780, -12, 1380));
        let corner = place(work, mon, TaskbarEdge::Bottom, None, SIZE, 1.0);
        assert_eq!(corner, p);
    }

    #[test]
    fn placement_clamped_inside_work_area() {
        let mon = r(0, 0, 1920, 1080);
        let work = r(0, 0, 1920, 1032);
        // Icon at the far right: the right edge keeps the 12 px gap.
        let p = place(
            work,
            mon,
            TaskbarEdge::Bottom,
            Some(r(1900, 1040, 1920, 1064)),
            SIZE,
            1.0,
        );
        assert_eq!(p, r(1548, 420, 1908, 1020));
        // Icon at the far left: still the tray corner.
        let p = place(
            work,
            mon,
            TaskbarEdge::Bottom,
            Some(r(0, 1040, 24, 1064)),
            SIZE,
            1.0,
        );
        assert_eq!(p, r(1548, 420, 1908, 1020));
        // A flyout taller than the work area is pinned to its top, never off screen.
        let small = r(0, 0, 1024, 500);
        let p = place(
            small,
            r(0, 0, 1024, 548),
            TaskbarEdge::Bottom,
            None,
            SIZE,
            1.0,
        );
        assert_eq!(p.top, 0);
        assert!(p.left >= small.left && p.right <= small.right);
    }

    #[test]
    fn placement_no_icon_uses_corner() {
        let mon = r(0, 0, 1920, 1080);
        let bottom = place(
            r(0, 0, 1920, 1032),
            mon,
            TaskbarEdge::Bottom,
            None,
            SIZE,
            1.0,
        );
        assert_eq!(bottom, r(1548, 420, 1908, 1020));
        let top = place(r(0, 48, 1920, 1080), mon, TaskbarEdge::Top, None, SIZE, 1.0);
        assert_eq!(top, r(1548, 60, 1908, 660));
        let left = place(
            r(60, 0, 1920, 1080),
            mon,
            TaskbarEdge::Left,
            None,
            SIZE,
            1.0,
        );
        assert_eq!(left, r(72, 468, 432, 1068));
        // An icon on another monitor (e.g. the overflow area elsewhere) counts as unknown.
        let elsewhere = r(-500, 1040, -476, 1064);
        let p = place(
            r(0, 0, 1920, 1032),
            mon,
            TaskbarEdge::Bottom,
            Some(elsewhere),
            SIZE,
            1.0,
        );
        assert_eq!(p, bottom);
    }
}
