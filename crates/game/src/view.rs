//! Camera and resolution-independent scaling.
//!
//! The original game ran at a fixed 800x600. Here the window can have any
//! size and aspect ratio: the scale is derived from the window *height*, so
//! the interface and the world appear exactly as tall as in the original,
//! and wider screens simply get a wider view of the map instead of a
//! stretched image.

/// Vertical resolution of the original game, used as the reference for the
/// automatic scale.
pub const REFERENCE_HEIGHT: f32 = 600.0;

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;

#[derive(Debug, Clone, Copy)]
pub struct ScalePolicy {
    /// Fixed screen pixels per original pixel; `None` derives it from the
    /// window height.
    pub fixed: Option<f32>,
    /// Snap the effective scale to whole numbers (crisp pixels, no uneven columns).
    pub integer: bool,
}

impl ScalePolicy {
    /// Screen pixels per original pixel (interface and unzoomed world) for a
    /// window `window_h` pixels tall.
    pub fn base_scale(&self, window_h: u32) -> f32 {
        let s = self
            .fixed
            .unwrap_or(window_h as f32 / REFERENCE_HEIGHT)
            .max(0.25);
        self.snap(s)
    }

    fn snap(&self, s: f32) -> f32 {
        if self.integer && s >= 1.0 {
            s.floor()
        } else {
            s
        }
    }
}

/// The part of the window showing the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    /// Viewport rectangle in window pixels.
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Height of the whole window, which determines the base scale.
    pub window_h: u32,
}

impl Screen {
    /// A viewport covering the whole window.
    pub fn full(w: u32, h: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            w,
            h,
            window_h: h,
        }
    }

    pub fn contains(&self, sx: f32, sy: f32) -> bool {
        sx >= self.x as f32
            && sy >= self.y as f32
            && sx < (self.x + self.w) as f32
            && sy < (self.y + self.h) as f32
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// World coordinates of the top-left corner of the view.
    pub x: f32,
    pub y: f32,
    /// User zoom factor applied on top of the base scale.
    pub zoom: f32,
    pub policy: ScalePolicy,
}

impl Camera {
    pub fn new(policy: ScalePolicy) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
            policy,
        }
    }

    /// Screen pixels per world pixel.
    pub fn scale(&self, screen: &Screen) -> f32 {
        let base = self.policy.base_scale(screen.window_h);
        self.policy.snap(base * self.zoom)
    }

    /// Size of the visible world area.
    pub fn view_size(&self, screen: &Screen) -> (f32, f32) {
        let s = self.scale(screen);
        (screen.w as f32 / s, screen.h as f32 / s)
    }

    /// World position under window position `(sx, sy)`.
    pub fn screen_to_world(&self, screen: &Screen, sx: f32, sy: f32) -> (f32, f32) {
        let s = self.scale(screen);
        (
            self.x + (sx - screen.x as f32) / s,
            self.y + (sy - screen.y as f32) / s,
        )
    }

    /// Keeps the view inside the map, centering the map on any axis where
    /// the view is larger than the map.
    pub fn clamp(&mut self, screen: &Screen, map: (u32, u32)) {
        let (vw, vh) = self.view_size(screen);
        self.x = clamp_axis(self.x, vw, map.0 as f32);
        self.y = clamp_axis(self.y, vh, map.1 as f32);
    }

    pub fn scroll(&mut self, dx_screen: f32, dy_screen: f32, screen: &Screen, map: (u32, u32)) {
        let s = self.scale(screen);
        self.x += dx_screen / s;
        self.y += dy_screen / s;
        self.clamp(screen, map);
    }

    /// Multiplies the zoom by `factor`, keeping the world point under the
    /// window position `(sx, sy)` fixed.
    pub fn zoom_at(&mut self, factor: f32, sx: f32, sy: f32, screen: &Screen, map: (u32, u32)) {
        let (wx, wy) = self.screen_to_world(screen, sx, sy);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let s = self.scale(screen);
        self.x = wx - (sx - screen.x as f32) / s;
        self.y = wy - (sy - screen.y as f32) / s;
        self.clamp(screen, map);
    }

    pub fn center_on(&mut self, wx: f32, wy: f32, screen: &Screen, map: (u32, u32)) {
        let (vw, vh) = self.view_size(screen);
        self.x = wx - vw / 2.0;
        self.y = wy - vh / 2.0;
        self.clamp(screen, map);
    }
}

fn clamp_axis(pos: f32, view: f32, map: f32) -> f32 {
    if view >= map {
        (map - view) / 2.0
    } else {
        pos.clamp(0.0, map - view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: ScalePolicy = ScalePolicy {
        fixed: None,
        integer: false,
    };

    #[test]
    fn widescreen_shows_more_horizontally() {
        let cam = Camera::new(AUTO);
        let (w43, h43) = cam.view_size(&Screen::full(800, 600));
        let (w169, h169) = cam.view_size(&Screen::full(1920, 1080));
        assert_eq!((w43, h43), (800.0, 600.0));
        assert_eq!(h169, 600.0);
        assert!((w169 - 1066.67).abs() < 0.01);
    }

    #[test]
    fn integer_scale_snaps() {
        let cam = Camera::new(ScalePolicy {
            fixed: None,
            integer: true,
        });
        assert_eq!(cam.scale(&Screen::full(1920, 1080)), 1.0);
        assert_eq!(cam.scale(&Screen::full(3840, 2160)), 3.0);
    }

    #[test]
    fn viewport_offsets_and_scale() {
        let cam = Camera::new(AUTO);
        // A 400x300 viewport at (100, 50) in a 1200x900 window: scale 1.5.
        let screen = Screen {
            x: 100,
            y: 50,
            w: 400,
            h: 300,
            window_h: 900,
        };
        assert_eq!(cam.scale(&screen), 1.5);
        assert_eq!(cam.screen_to_world(&screen, 100.0, 50.0), (0.0, 0.0));
        assert_eq!(cam.view_size(&screen), (400.0 / 1.5, 200.0));
    }

    #[test]
    fn small_maps_are_centered() {
        let mut cam = Camera::new(AUTO);
        cam.clamp(&Screen::full(1920, 1080), (640, 2000));
        assert!(cam.x < 0.0);
        assert_eq!(cam.y, 0.0);
    }

    #[test]
    fn zoom_keeps_cursor_anchor() {
        let mut cam = Camera::new(AUTO);
        let map = (4000, 4000);
        let screen = Screen {
            x: 20,
            y: 10,
            w: 800,
            h: 600,
            window_h: 600,
        };
        cam.x = 1000.0;
        cam.y = 1000.0;
        let before = cam.screen_to_world(&screen, 400.0, 300.0);
        cam.zoom_at(2.0, 400.0, 300.0, &screen, map);
        let after = cam.screen_to_world(&screen, 400.0, 300.0);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
    }
}
