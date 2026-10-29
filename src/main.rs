mod scan;
mod treemap;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, ecolor::Hsva};

use scan::{Progress, Tree, format_size};
use treemap::{BoxKind, HEADER_HEIGHT, LaidOutBox};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Space Analyzer").with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    eframe::run_native("Space Analyzer", options, Box::new(|_cc| Ok(Box::new(App::new(initial)))))
}

struct ScanJob {
    root: PathBuf,
    progress: Arc<Progress>,
    result: Receiver<Tree>,
    started: Instant,
}

struct App {
    tree: Option<Tree>,
    /// The directory currently filling the treemap.
    view: usize,
    scan: Option<ScanJob>,
    layout: Vec<LaidOutBox>,
    layout_key: Option<(Rect, usize)>,
    /// Box index the open context menu refers to.
    menu_target: Option<usize>,
    scan_time: Duration,
    status: String,
}

impl App {
    fn new(initial: Option<PathBuf>) -> Self {
        let mut app = App {
            tree: None,
            view: Tree::ROOT,
            scan: None,
            layout: Vec::new(),
            layout_key: None,
            menu_target: None,
            scan_time: Duration::ZERO,
            status: String::new(),
        };
        if let Some(path) = initial {
            app.start_scan(path);
        }
        app
    }

    fn start_scan(&mut self, root: PathBuf) {
        if let Some(job) = &self.scan {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let thread_progress = progress.clone();
        let thread_root = root.clone();
        std::thread::spawn(move || {
            let _ = tx.send(scan::scan(&thread_root, &thread_progress));
        });
        self.scan = Some(ScanJob { root, progress, result: rx, started: Instant::now() });
        self.status.clear();
    }

    fn pick_and_scan(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.start_scan(path);
        }
    }

    fn set_view(&mut self, view: usize) {
        self.view = view;
        self.layout_key = None;
    }

    fn open_dir(&mut self, id: usize) {
        if let Some(tree) = &self.tree {
            let path = tree.path(id);
            if let Err(err) = open::that_detached(&path) {
                self.status = format!("Couldn't open {}: {err}", path.display());
            }
        }
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.scan else { return };
        match job.result.try_recv() {
            Ok(tree) => {
                self.scan_time = job.started.elapsed();
                self.tree = Some(tree);
                self.scan = None;
                self.set_view(Tree::ROOT);
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
            Err(mpsc::TryRecvError::Disconnected) => self.scan = None,
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("📂 Choose folder…").clicked() {
                self.pick_and_scan();
            }
            let Some(tree) = &self.tree else { return };
            if ui.button("⟳ Rescan").clicked() {
                let root = tree.root_path.clone();
                self.start_scan(root);
                return;
            }
            let parent = tree.dirs[self.view].parent;
            if ui.add_enabled(parent.is_some(), egui::Button::new("⬆ Up")).clicked() {
                self.set_view(parent.unwrap());
                return;
            }
            ui.separator();

            // Breadcrumbs from the scan root down to the current view.
            let mut target = None;
            for (i, id) in tree.ancestry(self.view).into_iter().enumerate() {
                if i > 0 {
                    ui.label("›");
                }
                if ui.link(&tree.dirs[id].name).clicked() {
                    target = Some(id);
                }
            }
            if let Some(id) = target {
                self.set_view(id);
            }
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(tree) = &self.tree {
                let dir = &tree.dirs[self.view];
                ui.label(format!(
                    "{} in this view · {} folders scanned in {:.1}s",
                    format_size(dir.size),
                    tree.dirs.len(),
                    self.scan_time.as_secs_f32()
                ));
                ui.separator();
                ui.weak("Click: open in Finder · ⌘-click: zoom in · Right-click: more · Backspace: up");
            }
            if !self.status.is_empty() {
                ui.separator();
                ui.colored_label(Color32::LIGHT_RED, &self.status);
            }
        });
    }

    fn scanning_ui(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.scan else { return };
        let p = &job.progress;
        let mut cancel = false;
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.35);
            ui.heading(format!("Scanning {}", job.root.display()));
            ui.add_space(8.0);
            ui.spinner();
            ui.label(format!(
                "{} folders · {} files · {} · {:.0}s",
                p.dirs.load(Ordering::Relaxed),
                p.files.load(Ordering::Relaxed),
                format_size(p.bytes.load(Ordering::Relaxed)),
                job.started.elapsed().as_secs_f32()
            ));
            if let Ok(current) = p.current.lock() {
                ui.weak(current.as_str());
            }
            ui.add_space(8.0);
            cancel = ui.button("Cancel").clicked();
        });
        if cancel {
            p.cancel.store(true, Ordering::Relaxed);
            self.scan = None;
        }
    }

    fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.38);
            ui.heading("Space Analyzer");
            ui.label("Pick a folder to see what's taking up space.");
            ui.add_space(12.0);
            if ui.button("📂 Choose folder…").clicked() {
                self.pick_and_scan();
            }
            if let Some(home) = std::env::home_dir()
                && ui.button(format!("Scan home folder ({})", home.display())).clicked()
            {
                self.start_scan(home);
            }
        });
    }

    fn treemap_ui(&mut self, ui: &mut egui::Ui) {
        let Some(tree) = &self.tree else { return };
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click());
        let rect = response.rect;

        if self.layout_key != Some((rect, self.view)) {
            self.layout = treemap::layout(tree, self.view, rect);
            self.layout_key = Some((rect, self.view));
        }

        let hovered = response
            .hover_pos()
            .and_then(|pos| self.layout.iter().rposition(|b| b.rect.contains(pos)));

        for (i, b) in self.layout.iter().enumerate() {
            paint_box(&painter, tree, b, hovered == Some(i));
        }

        if let Some(i) = hovered {
            let b = &self.layout[i];
            let dir = &tree.dirs[b.dir];
            let path = tree.path(b.dir);
            response.clone().on_hover_ui_at_pointer(|ui| match b.kind {
                BoxKind::Dir => {
                    ui.strong(path.display().to_string());
                    ui.label(format!("{} · {} subfolders", format_size(dir.size), dir.children.len()));
                }
                BoxKind::Files => {
                    ui.strong(format!("{} files in {}", dir.file_count, path.display()));
                    ui.label(format_size(dir.files_size));
                }
            });
        }

        let mut action = None;
        if response.clicked()
            && let Some(i) = hovered
        {
            let zoom = ui.input(|i| i.modifiers.command);
            action = Some(if zoom { Action::Zoom(self.layout[i].dir) } else { Action::Open(self.layout[i].dir) });
        }
        if response.secondary_clicked() {
            self.menu_target = hovered;
        }
        if let Some(i) = self.menu_target.filter(|&i| i < self.layout.len()) {
            let id = self.layout[i].dir;
            response.context_menu(|ui| {
                ui.label(tree.dirs[id].name.as_str());
                ui.separator();
                if ui.button("Open in Finder").clicked() {
                    action = Some(Action::Open(id));
                    ui.close();
                }
                if ui.button("Zoom into folder").clicked() {
                    action = Some(Action::Zoom(id));
                    ui.close();
                }
                if ui.button("Copy path").clicked() {
                    ui.ctx().copy_text(tree.path(id).display().to_string());
                    ui.close();
                }
            });
        }

        if ui.input(|i| i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Escape))
            && let Some(parent) = tree.dirs[self.view].parent
        {
            action = Some(Action::Zoom(parent));
        }

        match action {
            Some(Action::Open(id)) => self.open_dir(id),
            Some(Action::Zoom(id)) => self.set_view(id),
            None => {}
        }
    }
}

enum Action {
    Open(usize),
    Zoom(usize),
}

fn paint_box(painter: &egui::Painter, tree: &Tree, b: &LaidOutBox, hovered: bool) {
    let shade = (0.88 - b.depth as f32 * 0.07).max(0.35);
    let fill: Color32 = match (b.kind, b.depth) {
        (BoxKind::Dir, 0) => Color32::from_gray(60),
        (BoxKind::Dir, _) => Hsva::new(b.hue, 0.5, shade, 1.0).into(),
        (BoxKind::Files, _) => Hsva::new(b.hue, 0.25, (shade + 0.08).min(0.95), 1.0).into(),
    };
    painter.rect_filled(b.rect, 2.0, fill);
    painter.rect_stroke(b.rect, 2.0, Stroke::new(1.0, Color32::from_black_alpha(90)), StrokeKind::Inside);

    let text_color = if b.depth == 0 || shade < 0.55 { Color32::WHITE } else { Color32::BLACK };
    let dir = &tree.dirs[b.dir];
    let clipped = painter.with_clip_rect(b.rect.shrink(2.0).intersect(painter.clip_rect()));
    let font = FontId::proportional(11.0);
    match b.kind {
        BoxKind::Dir if b.has_header => {
            let label = format!("{}  {}", dir.name, format_size(dir.size));
            clipped.text(b.rect.min + egui::vec2(4.0, 2.0), Align2::LEFT_TOP, label, font, text_color);
        }
        BoxKind::Files if b.rect.width() > 50.0 && b.rect.height() > HEADER_HEIGHT * 2.0 => {
            let label = format!("{} files\n{}", dir.file_count, format_size(dir.files_size));
            clipped.text(b.rect.center(), Align2::CENTER_CENTER, label, font, text_color);
        }
        _ => {}
    }

    if hovered {
        painter.rect_stroke(b.rect, 2.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_scan(ui.ctx());

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(4.0)).show(ui, |ui| {
            if self.scan.is_some() {
                self.scanning_ui(ui);
            } else if self.tree.is_some() {
                self.treemap_ui(ui);
            } else {
                self.welcome_ui(ui);
            }
        });
    }
}
