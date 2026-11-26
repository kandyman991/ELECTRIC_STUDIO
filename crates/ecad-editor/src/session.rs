//! A whole editing session as a shell sees it: the editor, the viewport, the project folder,
//! and input in screen pixels. Both UI shells drive this same type, so pan, zoom, key bindings
//! and saving behave identically in each; a shell only forwards [`UiEvent`]s and paints the
//! [`Frame`] it gets back. Both types are serialisable for shells that talk over IPC (Tauri).

use crate::view::Viewport;
use crate::{Editor, Key, Overlay, Primitive, Selected, Tool};
use ecad_model::{Point, SymbolId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hit tolerance in screen pixels; converted to millimetres at the current zoom.
const HIT_PX: f64 = 7.0;
/// Margin around the page when fitting it to the window.
const FIT_MARGIN_PX: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Primary,
    /// Middle button drags pan the view.
    Middle,
}

/// Input from a shell, in screen pixels relative to the canvas's top-left corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiEvent {
    Resize {
        width: f64,
        height: f64,
    },
    PointerMove {
        x: f64,
        y: f64,
    },
    PointerDown {
        x: f64,
        y: f64,
        button: Button,
    },
    PointerUp {
        x: f64,
        y: f64,
        button: Button,
    },
    /// Mouse wheel: positive `delta` zooms in.
    Wheel {
        x: f64,
        y: f64,
        delta: f64,
    },
    Key {
        key: KeyName,
    },
    /// `select`, `wire`, or a symbol id to place.
    Tool {
        tool: String,
    },
    Rename {
        tag: String,
    },
    Save,
    Fit,
}

/// Keys a shell maps from its own key events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyName {
    Escape,
    Delete,
    Rotate,
    Undo,
    Redo,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaletteItem {
    pub id: String,
    pub description: String,
}

/// Everything a shell needs to paint one frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub revision: u64,
    /// The page in page millimetres; `None` when the shell already has this revision.
    pub base: Option<Vec<Primitive>>,
    pub overlays: Vec<Overlay>,
    pub view: Viewport,
    pub status: String,
    /// `select`, `wire`, or the symbol id being placed.
    pub tool: String,
    pub selected_tag: Option<String>,
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub dirty: bool,
    pub title: String,
    pub palette: Vec<PaletteItem>,
}

#[derive(Debug)]
pub struct Session {
    editor: Editor,
    view: Viewport,
    dir: PathBuf,
    panning: Option<(f64, f64)>,
    fitted: bool,
    message: Option<String>,
}

impl Session {
    /// Open the project in `dir`, or create the sample project there when the folder has none.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let project = if dir.join("project.json").exists() {
            ecad_project::load(dir).map_err(|e| e.to_string())?
        } else {
            let p = ecad_model::sample::start_circuit().map_err(|e| e.to_string())?;
            ecad_project::save(&p, dir).map_err(|e| e.to_string())?;
            p
        };
        let page = *project.pages.keys().next().ok_or("the project has no pages")?;
        let editor = Editor::new(project, page).map_err(|e| e.to_string())?;
        Ok(Session {
            editor,
            view: Viewport::default(),
            dir: dir.to_owned(),
            panning: None,
            fitted: false,
            message: None,
        })
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }
    pub fn view(&self) -> &Viewport {
        &self.view
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn page_point(&mut self, x: f64, y: f64) -> Point {
        self.editor.hit_tolerance = HIT_PX / self.view.scale;
        self.view.to_page(x, y)
    }

    fn fit(&mut self) {
        if let Some((lo, hi)) = self.editor.extents() {
            self.view.fit(lo, hi, FIT_MARGIN_PX);
        }
    }

    pub fn handle(&mut self, ev: UiEvent) {
        self.message = None;
        match ev {
            UiEvent::Resize { width, height } => {
                self.view.width = width.max(1.0);
                self.view.height = height.max(1.0);
                if !self.fitted {
                    self.fit();
                    self.fitted = true;
                }
            }
            UiEvent::PointerMove { x, y } => {
                if let Some((px, py)) = self.panning {
                    self.view.pan(x - px, y - py);
                    self.panning = Some((x, y));
                } else {
                    let p = self.page_point(x, y);
                    self.editor.pointer_move(p);
                }
            }
            UiEvent::PointerDown { x, y, button: Button::Middle } => self.panning = Some((x, y)),
            UiEvent::PointerUp { button: Button::Middle, .. } => self.panning = None,
            UiEvent::PointerDown { x, y, button: Button::Primary } => {
                let p = self.page_point(x, y);
                self.editor.pointer_down(p);
            }
            UiEvent::PointerUp { x, y, button: Button::Primary } => {
                let p = self.page_point(x, y);
                self.editor.pointer_up(p);
            }
            UiEvent::Wheel { x, y, delta } => self.view.zoom_at(x, y, 1.15_f64.powf(delta.clamp(-10.0, 10.0))),
            UiEvent::Key { key } => self.editor.key(match key {
                KeyName::Escape => Key::Escape,
                KeyName::Delete => Key::Delete,
                KeyName::Rotate => Key::Rotate,
                KeyName::Undo => Key::Undo,
                KeyName::Redo => Key::Redo,
            }),
            UiEvent::Tool { tool } => self.editor.set_tool(match tool.as_str() {
                "select" => Tool::Select,
                "wire" => Tool::Wire,
                symbol => Tool::Place(SymbolId::new(symbol)),
            }),
            UiEvent::Rename { tag } => {
                if let Err(e) = self.editor.rename_selected(tag.trim()) {
                    self.message = Some(e.to_string());
                }
            }
            UiEvent::Save => self.save(),
            UiEvent::Fit => self.fit(),
        }
    }

    pub fn save(&mut self) {
        self.message = Some(match ecad_project::save(self.editor.project(), &self.dir) {
            Ok(()) => {
                self.editor.mark_saved();
                format!("Saved to {}", self.dir.display())
            }
            Err(e) => format!("Save failed: {e}"),
        });
    }

    /// The frame to paint. `known_revision` is the revision whose base the shell already holds.
    pub fn frame(&self, known_revision: Option<u64>, background: [u8; 3]) -> Frame {
        let editor = &self.editor;
        let revision = editor.revision();
        let (base, base_error) = if known_revision == Some(revision) {
            (None, None)
        } else {
            match editor.base(background) {
                Ok(b) => (Some(b), None),
                Err(e) => (Some(Vec::new()), Some(e.to_string())),
            }
        };
        let project = editor.project();
        let selected_tag = match editor.selection() {
            Some(Selected::Function(f)) => {
                project.function(f).and_then(|f| project.device(f.device)).map(|d| d.tag.clone()).ok()
            }
            _ => None,
        };
        let status = base_error.or_else(|| self.message.clone()).unwrap_or_else(|| editor.status().to_owned());
        Frame {
            revision,
            base,
            overlays: editor.overlays(),
            view: self.view,
            status,
            tool: match editor.tool() {
                Tool::Select => "select".into(),
                Tool::Wire => "wire".into(),
                Tool::Place(s) => s.0.clone(),
            },
            selected_tag,
            undo: editor.undo_label().map(str::to_owned),
            redo: editor.redo_label().map(str::to_owned),
            dirty: editor.is_dirty(),
            title: format!("{}{}", project.name, if editor.is_dirty() { " *" } else { "" }),
            palette: project
                .symbols
                .values()
                .map(|s| PaletteItem { id: s.id.0.clone(), description: s.description.clone() })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (tempfile::TempDir, Session) {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::open(dir.path()).unwrap();
        s.handle(UiEvent::Resize { width: 800.0, height: 600.0 });
        (dir, s)
    }

    #[test]
    fn a_new_folder_gets_the_sample_and_the_view_fits_it() {
        let (dir, s) = session();
        assert!(dir.path().join("project.json").exists());
        let (lo, hi) = s.editor().extents().unwrap();
        let (x0, y0) = s.view().to_screen(lo);
        let (x1, y1) = s.view().to_screen(hi);
        assert!(x0 >= 0.0 && x1 <= 800.0 && y1 >= 0.0 && y0 <= 600.0, "whole page visible");
    }

    #[test]
    fn frames_carry_the_base_only_when_it_changed() {
        let (_dir, mut s) = session();
        let f = s.frame(None, [0, 0, 0]);
        assert!(f.base.as_ref().is_some_and(|b| !b.is_empty()));
        assert!(s.frame(Some(f.revision), [0, 0, 0]).base.is_none());
        s.handle(UiEvent::Tool { tool: "iec.coil".into() });
        let (x, y) = s.view().to_screen(Point::new(40.0, 40.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::PointerUp { x, y, button: Button::Primary });
        let g = s.frame(Some(f.revision), [0, 0, 0]);
        assert!(g.base.is_some() && g.dirty && g.title.ends_with('*'));
        assert_eq!(g.selected_tag.as_deref(), Some("-K2"));
    }

    #[test]
    fn save_writes_the_folder_and_clears_dirty() {
        let (dir, mut s) = session();
        s.handle(UiEvent::Tool { tool: "iec.coil".into() });
        let (x, y) = s.view().to_screen(Point::new(40.0, 40.0));
        s.handle(UiEvent::PointerDown { x, y, button: Button::Primary });
        s.handle(UiEvent::Save);
        assert!(!s.frame(None, [0, 0, 0]).dirty);
        assert!(ecad_project::load(dir.path()).unwrap().device_by_tag("-K2").is_some());
    }

    #[test]
    fn middle_drag_pans_and_wheel_zooms_without_editing() {
        let (_dir, mut s) = session();
        let before = *s.view();
        s.handle(UiEvent::PointerDown { x: 100.0, y: 100.0, button: Button::Middle });
        s.handle(UiEvent::PointerMove { x: 140.0, y: 100.0 });
        s.handle(UiEvent::PointerUp { x: 140.0, y: 100.0, button: Button::Middle });
        assert!((s.view().center.x - (before.center.x - 40.0 / before.scale)).abs() < 1e-9);
        s.handle(UiEvent::Wheel { x: 400.0, y: 300.0, delta: 1.0 });
        assert!(s.view().scale > before.scale);
        assert!(!s.editor().is_dirty());
    }

    #[test]
    fn events_round_trip_as_json_for_ipc_shells() {
        let ev: UiEvent = serde_json::from_str(r#"{"type":"pointerDown","x":1,"y":2,"button":"primary"}"#).unwrap();
        assert_eq!(ev, UiEvent::PointerDown { x: 1.0, y: 2.0, button: Button::Primary });
        let key: UiEvent = serde_json::from_str(r#"{"type":"key","key":"undo"}"#).unwrap();
        assert_eq!(key, UiEvent::Key { key: KeyName::Undo });
        let (_dir, s) = session();
        let json = serde_json::to_value(s.frame(None, [0, 0, 0])).unwrap();
        assert!(json["base"].as_array().is_some_and(|b| !b.is_empty()));
        assert_eq!(json["tool"], "select");
    }
}
