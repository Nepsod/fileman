use nptk::std::path::PathBuf;

use nptk::gpui::{Empty, Render};

#[derive(Clone)]
pub struct DraggedFilePaths {
    pub paths: Vec<PathBuf>,
}

impl Render for DraggedFilePaths {
    fn render(
        &mut self,
        _window: &mut nptk::gpui::Window,
        _cx: &mut nptk::gpui::Context<'_, Self>,
    ) -> impl nptk::gpui::IntoElement {
        Empty
    }
}
