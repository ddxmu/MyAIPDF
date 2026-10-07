//! Manual check, download and confirmed installation. The desktop supplies native callbacks;
//! tests use inert substitutes. No startup check, automatic save or document transmission.
use std::sync::Arc;

use crate::{Dialog, PrintCraftApp, theme, widgets};
pub use printcraft_update::{APP_VERSION, Package, RELEASES_PAGE, Release, is_newer};

pub type UpdateSource = Arc<dyn Fn() -> Result<Release, String> + Send + Sync>;
pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;
pub type DownloadSource = Arc<dyn Fn(&Release, Progress) -> Result<Package, String> + Send + Sync>;
/// Starts a helper which waits for this app to exit before replacing it.
pub type InstallSource = Arc<dyn Fn(&Package) -> Result<(), String> + Send + Sync>;

#[derive(Default)]
pub(crate) enum Check {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running(std::sync::mpsc::Receiver<Result<Release, String>>),
    Done(Result<Release, String>),
}

#[cfg(not(target_arch = "wasm32"))]
enum Event {
    Progress(u64, u64),
    Done(Result<Package, String>),
}

#[derive(Default)]
enum Download {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running {
        rx: std::sync::mpsc::Receiver<Event>,
        bytes: u64,
        total: u64,
    },
    Ready(Package),
    Error(String),
    Installing,
}

#[derive(Default)]
pub(crate) struct Updates {
    pub(crate) check: Check,
    download: Download,
    pub(crate) open: bool,
}

impl PrintCraftApp {
    pub fn check_for_updates(&mut self) {
        let Some(source) = self.update_source.clone() else { return };
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.updates.open = self.dialog != Some(Dialog::About);
            if matches!(self.updates.check, Check::Running(_)) || matches!(self.updates.download, Download::Running { .. } | Download::Installing) {
                return;
            }
            self.updates.download = Download::Idle;
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(source());
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            self.updates.check = Check::Running(rx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = source;
            self.open_url(RELEASES_PAGE);
        }
    }

    pub fn download_update(&mut self) {
        let Some(source) = self.update_downloader.clone() else { return };
        let Check::Done(Ok(release)) = &self.updates.check else { return };
        if !is_newer(&release.version, APP_VERSION) || release.asset.is_none() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if matches!(self.updates.download, Download::Running { .. } | Download::Installing) {
                return;
            }
            let release = release.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                let updates = tx.clone();
                let repaint = ctx.clone();
                let progress: Progress = Arc::new(move |bytes, total| {
                    let _ = updates.send(Event::Progress(bytes, total));
                    if let Some(ctx) = &repaint {
                        ctx.request_repaint();
                    }
                });
                let _ = tx.send(Event::Done(source(&release, progress)));
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            self.updates.download = Download::Running { rx, bytes: 0, total: 0 };
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (source, release);
        }
    }

    pub fn install_update(&mut self) {
        if self.first_dirty().is_some() {
            self.notify("请先保存或另存为所有未保存的 PDF，再安装更新。");
            return;
        }
        let (Some(install), Download::Ready(package)) = (self.update_installer.clone(), &self.updates.download) else { return };
        match install(package) {
            Ok(()) => {
                self.updates.download = Download::Installing;
                if let Some(ctx) = &self.ctx {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            Err(e) => self.updates.download = Download::Error(e),
        }
    }

    pub(crate) fn install_update_enabled(&self) -> bool {
        self.update_installer.is_some() && matches!(self.updates.download, Download::Ready(_))
    }

    pub(crate) fn poll_updates(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Check::Running(rx) = &self.updates.check {
                match rx.try_recv() {
                    Ok(r) => self.updates.check = Check::Done(r),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => self.updates.check = Check::Done(Err("更新检查意外中断，请重试".into())),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
            let mut completed = None;
            if let Download::Running { rx, bytes, total } = &mut self.updates.download {
                for _ in 0..64 {
                    match rx.try_recv() {
                        Ok(Event::Progress(n, size)) => {
                            *bytes = n;
                            *total = size;
                        }
                        Ok(Event::Done(r)) => {
                            completed = Some(r);
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            completed = Some(Err("下载意外中断，请重试".into()));
                            break;
                        }
                    }
                }
                if let Some(ctx) = &self.ctx {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
            if let Some(result) = completed {
                self.updates.download = match result {
                    Ok(p) => Download::Ready(p),
                    Err(e) => Download::Error(e),
                };
            }
        }
    }

    /// Non-secret status for the UI and its opt-in control channel.
    pub fn update_status(&self) -> String {
        match &self.updates.download {
            #[cfg(not(target_arch = "wasm32"))]
            Download::Running { bytes, total, .. } => {
                return format!("正在下载更新：{:.1} / {:.1} MB", *bytes as f64 / 1048576.0, *total as f64 / 1048576.0);
            }
            Download::Ready(_) => return "安装包已下载并通过校验，点击“安装并重启”。".into(),
            Download::Error(e) => return format!("更新未完成：{e}"),
            Download::Installing => return "正在退出并安装更新，完成后将重新打开。".into(),
            Download::Idle => {}
        }
        match &self.updates.check {
            Check::Idle => "点击“检查更新”从 GitHub 获取最新版本。".into(),
            #[cfg(not(target_arch = "wasm32"))]
            Check::Running(_) => "正在检查新版…".into(),
            Check::Done(Ok(r)) if is_newer(&r.version, APP_VERSION) => format!("发现 MyAIPDF 新版本 {}（当前 {APP_VERSION}）。", r.version),
            Check::Done(Ok(_)) => format!("MyAIPDF 已是最新版本（{APP_VERSION}）。"),
            Check::Done(Err(e)) => format!("无法检查更新：{e}"),
        }
    }
}

/// Shared by Help/About and the separate manual-check dialog.
pub(crate) fn controls(app: &mut PrintCraftApp, ui: &mut egui::Ui) {
    let t = theme::Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(app.update_status()).color(t.text_muted));
    if app.first_dirty().is_some() {
        ui.label(egui::RichText::new("安装前请先保存所有未保存的 PDF。").color(t.text_muted).small());
    }
    if let Check::Done(Ok(release)) = &app.updates.check
        && is_newer(&release.version, APP_VERSION)
        && release.asset.is_none()
    {
        ui.label("此版本没有可校验的 Mac 安装包，请打开 GitHub 发布页手动下载。");
    }
    ui.horizontal(|ui| {
        let checking = matches!(app.updates.check, Check::Idle | Check::Done(_))
            && matches!(app.updates.download, Download::Idle | Download::Ready(_) | Download::Error(_));
        if ui.add_enabled_ui(checking && app.update_source.is_some(), |ui| widgets::pill_button(ui, "检查更新", false)).inner.clicked() {
            app.check_for_updates();
        }
        let newer = matches!(&app.updates.check, Check::Done(Ok(r)) if is_newer(&r.version, APP_VERSION) && r.asset.is_some());
        if matches!(app.updates.download, Download::Idle | Download::Error(_))
            && newer
            && app.update_downloader.is_some()
            && widgets::pill_button(ui, "下载安装包", true).clicked()
        {
            app.download_update();
        }
        if app.install_update_enabled()
            && ui.add_enabled_ui(app.first_dirty().is_none(), |ui| widgets::pill_button(ui, "安装并重启", true)).inner.clicked()
        {
            app.install_update();
        }
        ui.hyperlink_to("GitHub 发布页", RELEASES_PAGE);
    });
    ui.label(egui::RichText::new("仅手动检查；校验后安装，保留旧程序备份与个人设置。").small().color(t.text_muted));
}

pub(crate) fn dialog(app: &mut PrintCraftApp, ctx: &egui::Context) {
    if !app.updates.open || app.dialog == Some(Dialog::About) {
        return;
    }
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("updates")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(egui::RichText::new("MyAIPDF 软件更新").font(theme::semibold(18.0)));
        ui.add_space(12.0);
        controls(app, ui);
        ui.add_space(12.0);
        close = widgets::pill_button(ui, "Close", false).clicked();
    });
    if close || modal.should_close() {
        app.updates.open = false;
    }
}
