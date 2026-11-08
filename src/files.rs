//! File dialogs. On a computer these are the system's own, through `rfd`. A phone has
//! none, so there the same calls go to Android's picker, see `src/android.rs`.

#[cfg(not(target_os = "android"))]
pub use rfd::FileDialog;

#[cfg(not(target_os = "android"))]
/// Called once an export is written. Only a phone has more to do.
pub fn finished_writing(_path: &std::path::Path) {}

#[cfg(target_os = "android")]
pub use phone::{FileDialog, finished_writing};

#[cfg(target_os = "android")]
mod phone {
    use std::path::{Path, PathBuf};

    use crate::android;

    /// The parts of `rfd::FileDialog` Kaye uses, answered by the phone.
    #[derive(Default)]
    pub struct FileDialog {
        title: String,
        extensions: Vec<String>,
        file_name: Option<String>,
    }

    impl FileDialog {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn set_title(mut self, title: impl Into<String>) -> Self {
            self.title = title.into();
            self
        }

        pub fn add_filter(mut self, _name: impl Into<String>, extensions: &[impl ToString]) -> Self {
            self.extensions.extend(extensions.iter().map(|e| e.to_string()));
            self
        }

        pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
            self.file_name = Some(name.into());
            self
        }

        fn is_project(&self) -> bool {
            self.extensions.iter().any(|e| e == crate::project::EXTENSION)
        }

        /// Waits for the person, so it's only called from worker threads.
        pub fn pick_file(self) -> Option<PathBuf> {
            android::pick_files(false, self.is_project()).into_iter().next()
        }

        /// Waits for the person, so it's only called from worker threads.
        pub fn pick_files(self) -> Option<Vec<PathBuf>> {
            let picked = android::pick_files(true, self.is_project());
            (!picked.is_empty()).then_some(picked)
        }

        /// A project is kept in Kaye's own projects folder, under a name the person types.
        /// Anything else is an export: it's written inside Kaye first, then
        /// `finished_writing` asks where the copy goes, since the window can't wait here.
        pub fn save_file(self) -> Option<PathBuf> {
            let project = self.is_project();
            let name = self.file_name.unwrap_or_else(|| "Untitled".to_string());
            if project {
                let stem = Path::new(&name).file_stem().map_or_else(|| name.clone(), |s| s.to_string_lossy().into_owned());
                let typed = android::ask_name(&self.title, &stem)?;
                let clean: String = typed.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
                let folder = android::projects_dir();
                let _ = std::fs::create_dir_all(&folder);
                return Some(folder.join(format!("{clean}.{}", crate::project::EXTENSION)));
            }
            let folder = android::export_dir();
            let _ = std::fs::create_dir_all(&folder);
            Some(folder.join(name))
        }
    }

    /// Hands a written export to Android, which asks where to put it.
    pub fn finished_writing(path: &Path) {
        if path.starts_with(android::export_dir()) {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            android::save_copy(path, &name);
        }
    }
}
