fn main() {
    // Windows 릴리스 빌드는 tauri.windows.conf.json이 디버그 심볼(pstmacro.pdb)을 번들 리소스로
    // 포함한다(#199 — release backtrace 심볼화). 그런데 이 .pdb는 링커가 "이 빌드 스크립트 이후"에
    // 만들기 때문에, fresh 빌드에서는 tauri_build의 리소스 존재 검사가 "파일 없음"으로 실패한다
    // (닭-달걀). 검사 전에 빈 자리표시 .pdb를 만들어 두면 검사를 통과하고, 실제 .pdb가 링크
    // 시점에 그 자리를 덮어쓴다(자리표시는 흔적 없이 대체됨).
    //
    // ⚠️ 대상 OS 판정은 **호스트가 아니라 빌드 타깃**으로 해야 한다. 빌드 스크립트는 호스트에서
    // 실행되므로 `#[cfg(target_os="windows")]`는 리눅스(WSL) 크로스컴파일(cargo-xwin)에서 항상
    // false가 되어 placeholder가 안 만들어졌다 → fresh 타깃이 무조건 실패했다. 카고가 주는
    // `CARGO_CFG_TARGET_OS`(빌드 대상 OS)로 봐야 크로스컴파일에서도 동작한다.
    let target_is_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
    if target_is_windows && is_release {
        let pdb = std::path::Path::new("target/x86_64-pc-windows-msvc/release/pstmacro.pdb");
        if !pdb.exists() {
            if let Some(parent) = pdb.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::File::create(pdb);
        }
    }
    tauri_build::build()
}
