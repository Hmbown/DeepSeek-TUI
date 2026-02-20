use ratatui::{buffer::Buffer, layout::Rect};

pub trait Renderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn desired_height(&self, width: u16) -> u16;
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}
