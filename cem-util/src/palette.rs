use palette::{
    LinSrgb,
    LinSrgba,
    Srgb,
    Srgba,
};

pub trait ColorExt {
    const WHITE: Self;
    const BLACK: Self;

    fn gray(gray: f32) -> Self;

    /// Scales the color components, and leaves alpha untouched
    fn scaled_color(self, scale: f32) -> Self;
}

pub trait ColorAlphaExt {
    const TRANSPARENT: Self;
}

impl ColorExt for Srgba {
    const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
    const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    fn gray(gray: f32) -> Self {
        Self::new(gray, gray, gray, 1.0)
    }

    fn scaled_color(self, scale: f32) -> Self {
        let mut lin: LinSrgba = self.into_linear();
        lin.color *= scale;
        Srgba::from_linear(lin)
    }
}

impl ColorAlphaExt for Srgba {
    const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
}

impl ColorExt for LinSrgba {
    const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
    const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    fn gray(gray: f32) -> Self {
        Self::new(gray, gray, gray, 1.0)
    }

    fn scaled_color(mut self, scale: f32) -> Self {
        self.color *= scale;
        self
    }
}

impl ColorAlphaExt for LinSrgba {
    const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
}

impl ColorExt for Srgb {
    const WHITE: Self = Self::new(1.0, 1.0, 1.0);
    const BLACK: Self = Self::new(0.0, 0.0, 0.0);

    fn gray(gray: f32) -> Self {
        Self::new(gray, gray, gray)
    }

    fn scaled_color(self, scale: f32) -> Self {
        let lin: LinSrgb = self.into_linear();
        Srgb::from_linear(lin * scale)
    }
}

impl ColorExt for LinSrgb {
    const WHITE: Self = Self::new(1.0, 1.0, 1.0);
    const BLACK: Self = Self::new(0.0, 0.0, 0.0);

    fn gray(gray: f32) -> Self {
        Self::new(gray, gray, gray)
    }

    fn scaled_color(self, scale: f32) -> Self {
        self * scale
    }
}
