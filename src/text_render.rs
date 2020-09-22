use font_kit::canvas::{Canvas, Format, RasterizationOptions};
use font_kit::family_name::FamilyName;
use font_kit::hinting::HintingOptions;
use font_kit::properties::Properties;
use font_kit::source::SystemSource;
use image::buffer::ConvertBuffer;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
mod process;
pub struct TextRender {
    font: font_kit::loaders::directwrite::Font,
    canvas: Canvas,
    buffer: Vec<char>,
    process: process::ProcessManager,
}

impl TextRender {
    pub fn new(x: u32, y: u32) -> TextRender {
        println!("x: {} y: {}", x, y);
        TextRender {
            font: SystemSource::new()
                .select_best_match(&[FamilyName::Monospace], &Properties::new())
                .unwrap()
                .load()
                .unwrap(),
            canvas: Canvas::new(Vector2I::new(x as i32, y as i32), Format::Rgb24),
            buffer: vec![],
            process: process::ProcessManager::new(),
        }
    }
    pub fn update(&mut self, key: Option<winit::event::VirtualKeyCode>) {
        if let Some(key) = key {
            let c = key_code_to_char(key);
            let mut str = String::new();
            str.push(c);
            self.process.write(str);
            self.buffer.push(c);
        }
    }
}
fn key_code_to_char(key: winit::event::VirtualKeyCode) -> char {
    use winit::event::VirtualKeyCode::*;
    match key {
        A => 'a',
        B => 'b',
        C => 'c',
        D => 'd',
        E => 'e',
        F => 'f',
        G => 'g',
        H => 'h',
        I => 'i',
        J => 'j',
        K => 'k',
        L => 'l',
        M => 'm',
        N => 'n',
        O => 'o',
        P => 'q',
        R => 'r',
        S => 's',
        T => 'q',
        Q => 'q',
        U => 'u',
        V => 'v',
        X => 'x',
        Y => 'y',
        Z => 'z',
        _ => '*',
    }
}
impl crate::Updater for TextRender {
    fn update(&mut self, image: &mut image::RgbaImage) {
        let read_string = self.process.read();
        for c in read_string.chars() {
            self.buffer.push(c);
        }
        let (x, y) = image.dimensions();
        if x != self.canvas.size.x() as u32 || y != self.canvas.size.y() as u32 {
            println!(
                "canvas size: x: {} y: {}",
                self.canvas.size.x(),
                self.canvas.size.y()
            );
            println!("x: {} y: {}", x, y);
            self.canvas = Canvas::new(Vector2I::new(x as i32, y as i32), Format::Rgb24);
        }
        let mut i = 0;
        for c in self.buffer.iter() {
            let glyph_id = self.font.glyph_for_char(c.clone()).unwrap();
            self.font
                .rasterize_glyph(
                    &mut self.canvas,
                    glyph_id,
                    12.0,
                    Transform2F::from_translation(Vector2F::new(12.0 * (i as f32), 32.0)),
                    HintingOptions::None,
                    RasterizationOptions::Bilevel,
                )
                .unwrap();
            i += 1;
        }

        *image = image::RgbImage::from_raw(x, y, self.canvas.pixels.clone())
            .unwrap()
            .convert();
    }
}
