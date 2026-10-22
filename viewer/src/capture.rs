//! Atomic, no-clobber publication of a completed local PNG. No upload/network.
use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temporary {
    path: PathBuf,
    file: File,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        // Only a name created exclusively by this invocation is removed.
        let _ = fs::remove_file(&self.path);
    }
}

fn publish_new(path: &Path, encode: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = None;
    for _ in 0..100 {
        let candidate = parent.join(format!(
            ".sm64-shot-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some(Temporary {
                    path: candidate,
                    file,
                });
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).context("creating exclusive screenshot temporary file");
            }
        }
    }
    let Some(mut temporary) = temporary else {
        bail!("screenshot temporary-name budget exhausted");
    };
    encode(&mut temporary.file)?;
    temporary
        .file
        .sync_all()
        .context("syncing complete screenshot")?;
    // A hard link publishes the already complete same-filesystem file atomically
    // without replacing an existing pathname, including a ROM alias or symlink.
    // Unlike ordinary rename, it preserves create-new semantics even after a race.
    fs::hard_link(&temporary.path,path).context("publishing new screenshot (destination must not exist; filesystem must support hard links)")?;
    Ok(())
}

pub fn write_png(path: &Path, image: &image::DynamicImage) -> Result<()> {
    publish_new(path, |file| {
        let mut writer = BufWriter::new(file);
        image
            .write_to(&mut writer, image::ImageFormat::Png)
            .context("encoding screenshot")?;
        writer.flush().context("flushing screenshot")?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "sm64-png-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("authored.png")
        }
        fn entries(&self) -> usize {
            fs::read_dir(&self.0).unwrap().count()
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_a_complete_png_is_published_and_existing_output_is_preserved() {
        let dir = Directory::new();
        let image = image::DynamicImage::new_rgba8(2, 3);
        write_png(&dir.path(), &image).unwrap();
        let bytes = fs::read(dir.path()).unwrap();
        let decoded = image::open(dir.path()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 3));
        assert_eq!(dir.entries(), 1);
        assert!(write_png(&dir.path(), &image).is_err());
        assert_eq!(fs::read(dir.path()).unwrap(), bytes);
        assert_eq!(dir.entries(), 1);
    }

    #[test]
    fn failed_encoding_and_late_destination_races_do_not_leave_partial_outputs() {
        let dir = Directory::new();
        let failed = publish_new(&dir.path(), |file| {
            file.write_all(b"authored partial output")?;
            bail!("injected encoder failure")
        });
        assert!(failed.is_err());
        assert!(!dir.path().exists());
        assert_eq!(dir.entries(), 0);
        let raced = publish_new(&dir.path(), |file| {
            file.write_all(b"authored new data")?;
            fs::write(dir.path(), b"keep this existing destination")?;
            Ok(())
        });
        assert!(raced.is_err());
        assert_eq!(
            fs::read(dir.path()).unwrap(),
            b"keep this existing destination"
        );
        assert_eq!(dir.entries(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_hardlink_aliases_never_modify_the_source() {
        let dir = Directory::new();
        let source = dir.0.join("source.z64");
        fs::write(&source, b"authored placeholder, not a ROM").unwrap();
        std::os::unix::fs::symlink(&source, dir.path()).unwrap();
        assert!(write_png(&dir.path(), &image::DynamicImage::new_rgba8(1, 1)).is_err());
        fs::remove_file(dir.path()).unwrap();
        fs::hard_link(&source, dir.path()).unwrap();
        assert!(write_png(&dir.path(), &image::DynamicImage::new_rgba8(1, 1)).is_err());
        assert_eq!(
            fs::read(source).unwrap(),
            b"authored placeholder, not a ROM"
        );
        assert_eq!(dir.entries(), 2);
    }
}
