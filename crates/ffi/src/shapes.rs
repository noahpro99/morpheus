//! The concentric rule (#504/#505) for the phone: the shared arithmetic of `view_model::concentric`, so Kotlin
//! and the desktop compute the same insets and radii.

use morpheus_view_model::concentric;

/// How far in from the container's edge, on both axes, a child of `size` sits so it is centred on the end-cap circle of a
/// container with corner radius `outer` (zero when the child is wider than the cap).
#[uniffi::export]
pub fn concentric_end_cap_inset(outer: f32, size: f32) -> f32 {
    concentric::end_cap_inset(outer, size)
}

/// The corner radius of a shape inset `inset` from a rounded container of radius `outer`.
#[uniffi::export]
pub fn concentric_inner_radius(outer: f32, inset: f32) -> f32 {
    concentric::inner_radius(outer, inset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_in_the_composers_pill_sits_four_dp_in() {
        // The composer's field is 56 dp tall with a 28 dp corner; its buttons are 48 dp.
        assert_eq!(concentric_end_cap_inset(28.0, 48.0), 4.0);
        assert_eq!(concentric_inner_radius(28.0, 4.0), 24.0);
    }
}
