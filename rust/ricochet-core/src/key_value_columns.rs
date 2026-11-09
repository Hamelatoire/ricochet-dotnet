//! Port of `src/Ricochet.Core/KeyValueColumns.cs` (ADR 0074). The split between the key and value columns of a
//! key-value table is a proportion of the space the two columns share, not pixels, so a width saved in a small
//! window still fits a larger one. The UI that draws the columns is not ported.

/// The key column's share when nothing was dragged: one part of three, the value column two.
pub const DEFAULT_KEY_SHARE: f64 = 1.0 / 3.0;

/// The narrowest either column may be, in device-independent units, so a few characters always show.
pub const MIN_COLUMN_WIDTH: f64 = 80.0;

/// Keeps a key share inside the range where both columns are at least [`MIN_COLUMN_WIDTH`] wide.
/// A value that is not a finite number, or a space too small for both minimums, gives [`DEFAULT_KEY_SHARE`].
pub fn clamp(share: f64, total_width: f64) -> f64 {
    if !share.is_finite() || !total_width.is_finite() || total_width < 2.0 * MIN_COLUMN_WIDTH {
        return DEFAULT_KEY_SHARE;
    }

    let low = MIN_COLUMN_WIDTH / total_width;
    let high = 1.0 - low;
    share.clamp(low, high)
}

/// True for a share that a saved file may hold: a finite proportion strictly between zero and one.
pub fn is_saveable(share: f64) -> bool {
    share.is_finite() && share > 0.0 && share < 1.0
}
