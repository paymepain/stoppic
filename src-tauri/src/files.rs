use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tauri_plugin_opener::OpenerExt;

pub fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string()
}

/// Новый файл «Имя.ext», затем «Имя (2).ext» и т.д. Существующие файлы не перезаписываются.
pub fn create_unique(dir: &Path, base: &str, ext: &str) -> io::Result<(File, PathBuf)> {
    fs::create_dir_all(dir)?;
    for n in 1..=99 {
        let name = if n == 1 { format!("{base}.{ext}") } else { format!("{base} ({n}).{ext}") };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("не удалось подобрать свободное имя файла"))
}

pub fn write_unique(dir: &Path, base: &str, ext: &str, data: &[u8]) -> io::Result<PathBuf> {
    let (mut f, path) = create_unique(dir, base, ext)?;
    if let Err(e) = f.write_all(data) {
        drop(f);
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// Перенос между дисками не работает через rename, тогда копируем.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    let _ = fs::remove_file(from);
    Ok(())
}

/// Папку можно создать и в неё можно писать.
pub fn usable_dir(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".stoppic-{}", std::process::id()));
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

pub fn reveal(path: PathBuf) -> crate::notify::OnClick {
    Box::new(move || {
        let _ = crate::app().opener().reveal_item_in_dir(&path);
    })
}

pub fn open_dir(dir: &Path) {
    if usable_dir(dir) {
        let _ = crate::app().opener().open_path(dir.to_string_lossy(), None::<&str>);
    }
}
