pub const MIN_REGION_SIZE: u32 = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub screen: Rect,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    // Coordinates are physical desktop pixels, including negative monitor origins.
    // H.264 output uses even dimensions. Only the right/bottom edge is shortened.
    pub fn select(start: (i32, i32), end: (i32, i32), monitor: Rect) -> Result<Self, String> {
        if monitor.left >= monitor.right || monitor.top >= monitor.bottom {
            return Err("モニターのサイズが不正です。".into());
        }
        let left = start.0.min(end.0).max(monitor.left);
        let top = start.1.min(end.1).max(monitor.top);
        let right = start.0.max(end.0).min(monitor.right);
        let bottom = start.1.max(end.1).min(monitor.bottom);
        let width = (i64::from(right) - i64::from(left)).max(0) as u32 & !1;
        let height = (i64::from(bottom) - i64::from(top)).max(0) as u32 & !1;
        if width < MIN_REGION_SIZE || height < MIN_REGION_SIZE {
            return Err("48 × 48 ピクセル以上の範囲を選択してください。".into());
        }
        Ok(Self {
            screen: Rect {
                left,
                top,
                right: left + width as i32,
                bottom: top + height as i32,
            },
            x: (i64::from(left) - i64::from(monitor.left)) as u32,
            y: (i64::from(top) - i64::from(monitor.top)) as u32,
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MONITOR: Rect = Rect {
        left: -1920,
        top: -200,
        right: 0,
        bottom: 880,
    };

    #[test]
    fn reverse_drag_on_negative_origin_monitor_preserves_physical_offsets() {
        let r = Region::select((-100, 601), (-701, 200), MONITOR).unwrap();
        assert_eq!((r.x, r.y, r.width, r.height), (1219, 400, 600, 400));
        assert_eq!(r.screen.right, -101);
        assert_eq!(r.screen.bottom, 600);
    }

    #[test]
    fn crossing_monitor_edge_is_clipped_to_initial_monitor() {
        let r = Region::select((-400, 500), (1000, 1100), MONITOR).unwrap();
        assert_eq!((r.x, r.y, r.width, r.height), (1520, 700, 400, 380));
    }

    #[test]
    fn empty_or_tiny_selection_is_rejected() {
        assert!(Region::select((-20, 0), (-20, 100), MONITOR).is_err());
        assert!(Region::select((-20, 0), (-5, 100), MONITOR).is_err());
        assert!(Region::select((100, 100), (200, 200), MONITOR).is_err());
        assert!(Region::select((-120, 0), (-73, 80), MONITOR).is_err());
    }
}
pub mod save;
