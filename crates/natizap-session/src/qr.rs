use qrcode::{Color, QrCode};

/// A QR code as a square grid of dark and light modules, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    width: usize,
    dark: Vec<bool>,
}

impl QrMatrix {
    /// Encodes `payload` as a QR code.
    ///
    /// # Errors
    ///
    /// Fails when the payload is too long to fit in any QR code version.
    pub fn encode(payload: &str) -> Result<Self, qrcode::types::QrError> {
        let code = QrCode::new(payload.as_bytes())?;
        let dark = code
            .to_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect();
        Ok(Self {
            width: code.width(),
            dark,
        })
    }

    /// Number of modules on each side of the square.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Iterates over the rows, top to bottom; each row yields `true` for dark
    /// modules, left to right.
    pub fn rows(&self) -> impl Iterator<Item = &[bool]> {
        self.dark.chunks(self.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_square_grid_with_finder_patterns() {
        let qr = QrMatrix::encode("2@pairing-ref,noise-key,identity-key,adv-secret")
            .unwrap_or_else(|e| panic!("payload should encode: {e}"));

        let rows: Vec<&[bool]> = qr.rows().collect();
        assert_eq!(rows.len(), qr.width());
        assert!(rows.iter().all(|row| row.len() == qr.width()));

        // Every QR code has a 7x7 finder pattern with a dark border in three
        // corners; check the top-left one.
        let last = qr.width() - 1;
        assert!(rows[0][0] && rows[0][6] && rows[6][0] && rows[6][6]);
        assert!(
            !rows[1][1],
            "finder pattern has a light ring inside the border"
        );
        assert!(rows[0][last], "top-right finder pattern");
        assert!(rows[last][0], "bottom-left finder pattern");
    }
}
