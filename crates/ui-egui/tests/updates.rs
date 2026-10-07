//! End-to-end manual MyAIPDF updates with inert network/install callbacks.
use egui_kittest::{Harness, kittest::Queryable};
use printcraft_ui_egui::updates::{APP_VERSION, Package, Release, UpdateSource, is_newer};
use printcraft_ui_egui::{Dialog, PrintCraftApp, i18n::Language};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn release(version: &str) -> Release {
    Release {
        version: version.trim_start_matches('v').into(),
        url: format!("https://github.com/ddxmu/MyAIPDF/releases/tag/{version}"),
        notes: "测试用新版说明".into(),
        asset: Some(printcraft_update::Asset {
            url: "https://github.com/ddxmu/MyAIPDF/releases/download/v99.0.0/MyAIPDF.dmg".into(),
            sha256: "a".repeat(64),
            size: 3,
        }),
    }
}
fn source(answer: Result<&str, &str>) -> UpdateSource {
    let answer = answer.map(release).map_err(str::to_string);
    Arc::new(move || answer.clone())
}
fn harness(answer: Result<&str, &str>) -> Harness<'static, PrintCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.language = Language::Zh;
        app.update_source = Some(source(answer));
        app.update_downloader = Some(Arc::new(|r, progress| {
            progress(3, 3);
            Ok(Package { path: "/inert/test/MyAIPDF.dmg".into(), version: r.version.clone(), sha256: "a".repeat(64), size: 3 })
        }));
        app
    })
}
fn settle(h: &mut Harness<'static, PrintCraftApp>) {
    for _ in 0..200 {
        h.run_steps(2);
        let status = h.state().update_status();
        if !status.contains("正在检查") && !status.contains("正在下载") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(3);
}

#[test]
fn versions_compare_by_number() {
    assert!(is_newer("v0.1.2", "0.1.1"));
    assert!(!is_newer(APP_VERSION, APP_VERSION));
    assert!(is_newer("v0.1.10", "0.1.9"));
    assert!(is_newer("1", "0.9.9"));
    for v in ["v0.1.1", "v0.1.0", "v0.1.1-beta.2", "nightly", "v1.2.3.4", "v99999999999999999999.0.0"] {
        assert!(!is_newer(v, APP_VERSION), "{v}");
    }
}
#[test]
fn a_newer_release_is_offered_for_download() {
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("发现 MyAIPDF 新版本 99.0.0");
    h.get_by_label("下载增量更新");
    assert!(h.query_by_label("安装并重启").is_none());
    h.get_by_label("关闭").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("发现 MyAIPDF 新版本").is_none());
}
#[test]
fn an_up_to_date_or_failed_check_says_so() {
    let mut h = harness(Ok(APP_VERSION));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("已是最新版本");
    assert!(h.query_by_label("下载增量更新").is_none());
    let mut h = harness(Err("测试网络不可用"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("无法检查更新：测试网络不可用");
}
#[test]
fn nothing_is_asked_until_the_user_checks() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().restore(r#"{"check_updates_at_start":true}"#);
    h.state_mut().update_source = Some(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(release("v99.0.0"))
    }));
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn about_contains_update_controls_and_confirmed_install() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().update_installer = Some(Arc::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    h.state_mut().execute("help.about");
    h.run_steps(4);
    assert_eq!(h.state().dialog, Some(Dialog::About));
    h.get_by_label("检查更新").click();
    settle(&mut h);
    h.get_by_label("下载增量更新").click();
    settle(&mut h);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "download does not authorize installing");
    h.get_by_label("安装并重启");
    if let Ok(path) = std::env::var("MYAIPDF_UPDATE_SHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    h.get_by_label("安装并重启").click();
    h.run_steps(3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn unsaved_documents_block_install_without_discarding_edits() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().update_installer = Some(Arc::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.state_mut().download_update();
    settle(&mut h);
    h.state_mut().open_bytes("synthetic.pdf", None, include_bytes!("data/form.pdf").to_vec()).unwrap();
    h.state_mut().apply_edit(printcraft_engine::Edit::RotatePages { pages: vec![0], degrees: 90 });
    h.state_mut().install_update();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(h.state().first_dirty().is_some());
    assert!(h.state().update_status().contains("增量包已下载"));
}
#[test]
fn failed_download_can_be_retried() {
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().update_downloader = Some(Arc::new(|_, _| Err("校验失败，未安装".into())));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.state_mut().download_update();
    settle(&mut h);
    h.get_by_label_contains("校验失败，未安装");
    h.get_by_label("下载增量更新");
    assert!(h.query_by_label("安装并重启").is_none());
}
