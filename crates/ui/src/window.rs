//! Window controls the platform crate hands to the shared UI. The desktop
//! app draws its own title bar (no native decorations), so dragging,
//! minimising, maximising and closing go through these callbacks; a platform
//! that keeps native chrome (mobile) provides none and the bar hides them.

use std::rc::Rc;

#[derive(Clone)]
pub struct WindowControls {
    pub drag: Rc<dyn Fn()>,
    pub minimize: Rc<dyn Fn()>,
    pub toggle_maximize: Rc<dyn Fn()>,
    pub close: Rc<dyn Fn()>,
}

impl std::fmt::Debug for WindowControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WindowControls")
    }
}
