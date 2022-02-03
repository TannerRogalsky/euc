use euc::{Buffer2d, Pipeline, TriangleList, CullMode, Empty};
use minifb::{Key, Window, WindowOptions};
use vek::*;

struct Triangle;

impl Pipeline for Triangle {
    type Vertex = [f32; 2];
    type VertexData = Vec2<f32>;
    type Primitives = TriangleList;
    type Fragment = Vec2<f32>;
    type Pixel = u32;

    fn vertex_shader(&self, pos: &[f32; 2]) -> ([f32; 4], Self::VertexData) {
        ([pos[0], pos[1], 0.0, 1.0], Vec2::new(pos[0], pos[1]))
    }

    fn fragment_shader(&self, xy: Self::VertexData) -> Self::Fragment { xy }

    fn blend_shader(&self, _: Self::Pixel, xy: Self::Fragment) -> Self::Pixel {
        u32::from_le_bytes([(xy.x * 255.0) as u8, (xy.y * 255.0) as u8, 255, 255])
    }
}
fn main() {
    let [w, h] = [640, 480];
    let mut color = Buffer2d::fill([w, h], 0);
    let mut win = Window::new("Triangle", w, h, WindowOptions::default()).unwrap();

    Triangle.render(
        &[[-1.0, -1.0], [1.0, -1.0], [0.0, 1.0]],
        CullMode::None,
        &mut color,
        &mut Empty::default(),
    );

    while win.is_open() && !win.is_key_down(Key::Escape) {
        win.update_with_buffer(color.raw(), w, h).unwrap();
    }
}
