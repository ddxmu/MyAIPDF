use super::*;
use serde_json::json;

fn release_json() -> Value {
    json!({"draft":false,"prerelease":false,"tag_name":"v0.1.4","html_url":format!("{RELEASES_PAGE}/tag/v0.1.4"),"body":"新版说明","assets":[{
        "name":delta_filename("0.1.4",APP_VERSION),"state":"uploaded","size":3,"digest":format!("sha256:{}","a".repeat(64)),
        "browser_download_url":format!("{RELEASES_PAGE}/download/v0.1.4/{}",delta_filename("0.1.4",APP_VERSION))
    }]})
}

#[test]
fn answers_are_read_and_only_our_stable_release_is_offered() {
    let value = release_json();
    let r = parse_release(&value.to_string()).unwrap();
    assert_eq!(r.version, "0.1.4");
    assert_eq!(r.notes, "新版说明");
    assert_eq!(r.asset.unwrap().sha256, "a".repeat(64));
    for (key, invalid) in [
        ("draft", json!(true)),
        ("prerelease", json!(true)),
        ("html_url", json!("https://github.com/other/app/releases/tag/v0.1.4")),
        ("tag_name", json!("../../nightly")),
    ] {
        let mut bad = value.clone();
        bad[key] = invalid;
        assert!(parse_release(&bad.to_string()).is_err(), "{key}");
    }
    for (key, invalid) in
        [("browser_download_url", json!("https://example.com/app.dmg")), ("size", json!(MAX_PACKAGE_BYTES + 1)), ("state", json!("starter"))]
    {
        let mut bad = value.clone();
        bad["assets"][0][key] = invalid;
        assert!(parse_release(&bad.to_string()).is_err(), "{key}");
    }
    let mut missing = value.clone();
    missing["assets"][0]["digest"] = Value::Null;
    assert!(parse_release(&missing.to_string()).unwrap().asset.is_none());
    let mut duplicate = value.clone();
    duplicate["assets"].as_array_mut().unwrap().push(value["assets"][0].clone());
    assert!(parse_release(&duplicate.to_string()).is_err());
    assert!(parse_release("not JSON").is_err());
}

#[test]
fn deltas_match_the_exact_base_and_never_fall_back_to_full_packages() {
    let mut value = release_json();
    value["assets"].as_array_mut().unwrap().push(json!({
        "name":"MyAIPDF.dmg","state":"uploaded","size":100,
        "digest":format!("sha256:{}","b".repeat(64)),
        "browser_download_url":format!("{RELEASES_PAGE}/download/v0.1.4/MyAIPDF.dmg")
    }));
    assert!(parse_release_for(&value.to_string(), APP_VERSION).unwrap().asset.is_some());
    assert!(parse_release_for(&value.to_string(), "0.1.1").unwrap().asset.is_none());
    value["assets"].as_array_mut().unwrap().remove(0);
    assert!(parse_release(&value.to_string()).unwrap().asset.is_none());
}

#[test]
fn corrupted_or_symlinked_packages_are_rejected_before_install() {
    let directory = private_directory(&std::env::temp_dir()).unwrap();
    let path = directory.join("MyAIPDF.dmg");
    std::fs::write(&path, b"abc").unwrap();
    let package = Package {
        path: path.clone(),
        version: "0.1.4".into(),
        size: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
    };
    verify_package(&package).unwrap();
    std::fs::write(&path, b"abd").unwrap();
    let target = directory.join("MyAIPDF.app");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("keep"), "old").unwrap();
    assert!(install(&package, &target).unwrap_err().contains("SHA-256"));
    assert_eq!(std::fs::read_to_string(target.join("keep")).unwrap(), "old");
    #[cfg(unix)]
    {
        let linked = directory.join("linked.dmg");
        std::os::unix::fs::symlink(&path, &linked).unwrap();
        assert!(verify_package(&Package { path: linked, ..package }).is_err());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn check_has_no_authentication_or_pdf_payload() {
    use std::io::{Read, Write};
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 10000 {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /latest "));
        assert!(!request.contains("authorization:") && !request.contains("api-key") && !request.contains("content-length:"));
        // MyAIPDF is only the product User-Agent, not document contents.
        let body = release_json().to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
            .unwrap();
    });
    assert_eq!(check_url(&format!("http://{address}/latest")).unwrap().version, "0.1.4");
    worker.join().unwrap();
}
