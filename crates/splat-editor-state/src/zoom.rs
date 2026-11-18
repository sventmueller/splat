//! How large both views show the text, as a factor of the normal size. All windows share it, and the next start keeps it.

/// The size the text has without a zoom.
pub const NORMAL: f64 = 1.0;
/// One step of the View menu; every zoom it reaches is a power of it, so that steps back return exactly.
const STEP: f64 = 1.1;
const SMALLEST: f64 = 0.5;
const LARGEST: f64 = 3.0;

/// The zoom one step larger than `zoom`.
pub fn larger(zoom: f64) -> f64 {
    stepped(zoom, 1.0)
}

/// The zoom one step smaller than `zoom`.
pub fn smaller(zoom: f64) -> f64 {
    stepped(zoom, -1.0)
}

/// `stored` as a zoom: what no step reaches, such as the 0 of a setting never made, is the normal size.
pub fn kept(stored: f64) -> f64 {
    if (SMALLEST..=LARGEST).contains(&stored) {
        stored
    } else {
        NORMAL
    }
}

fn stepped(zoom: f64, by: f64) -> f64 {
    let level = (kept(zoom).ln() / STEP.ln()).round();
    STEP.powf(level + by).clamp(SMALLEST, LARGEST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_in_and_out_come_back_to_the_normal_size_exactly() {
        let mut zoom = NORMAL;
        for _ in 0..4 {
            zoom = larger(zoom);
        }
        assert!((zoom - 1.4641).abs() < 1e-9, "{zoom}");
        for _ in 0..4 {
            zoom = smaller(zoom);
        }
        assert_eq!(zoom, NORMAL);
    }

    #[test]
    fn the_zoom_stays_between_half_and_three_times_the_size() {
        let mut zoom = NORMAL;
        for _ in 0..30 {
            zoom = larger(zoom);
        }
        assert_eq!(zoom, LARGEST);
        assert!(smaller(LARGEST) < LARGEST);
        for _ in 0..30 {
            zoom = smaller(zoom);
        }
        assert_eq!(zoom, SMALLEST);
        assert!(larger(SMALLEST) > SMALLEST);
    }

    #[test]
    fn a_stored_value_no_step_reaches_means_the_normal_size() {
        assert_eq!(kept(0.0), NORMAL, "never set");
        assert_eq!(kept(f64::NAN), NORMAL);
        assert_eq!(kept(12.0), NORMAL);
        assert_eq!(kept(1.21), 1.21);
        assert_eq!(larger(f64::NAN), STEP);
    }
}
