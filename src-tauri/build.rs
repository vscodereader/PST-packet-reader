fn main() {
    // Windows 릴리스 빌드는 tauri.windows.conf.json이 디버그 심볼(pstmacro.pdb)을 번들 리소스로
    // 포함한다(#199 — release backtrace 심볼화). 그런데 이 .pdb는 링커가 "이 빌드 스크립트 이후"에
    // 만들기 때문에, fresh 빌드에서는 tauri_build의 리소스 존재 검사가 "파일 없음"으로 실패한다
    // (닭-달걀). 검사 전에 빈 자리표시 .pdb를 만들어 두면 검사를 통과하고, 실제 .pdb가 링크
    // 시점에 그 자리를 덮어쓴다(자리표시는 흔적 없이 대체됨). 동기가 리눅스에서 빌드할 땐 이
    // 윈도우 전용 설정이 적용되지 않아 무관하다.
    #[cfg(target_os = "windows")]
    {
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
