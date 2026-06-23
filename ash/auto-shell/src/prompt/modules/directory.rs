//! Directory module — shows current working directory with ~ abbreviation

use crate::prompt::context::AshContext;
use crate::prompt::module::{PromptModule, PromptSegment, SegmentStyle};
use nu_ansi_term::Color;

pub struct DirectoryModule {
    style: SegmentStyle,
    home_symbol: String,
}

impl DirectoryModule {
    pub fn new(config: &super::super::config::AshConfig) -> Self {
        Self {
            style: SegmentStyle {
                fg: Some(Color::Cyan),
                bold: true,
                ..Default::default()
            },
            home_symbol: config
                .module_string("directory", "home_symbol", "~")
                .to_string(),
        }
    }
}

impl PromptModule for DirectoryModule {
    fn name(&self) -> &str {
        "directory"
    }

    fn render(&self, ctx: &AshContext) -> Option<PromptSegment> {
        let mut dir_str = ctx.cwd.to_string_lossy().to_string();

        // Normalize separators to forward slash
        dir_str = dir_str.replace('\\', "/");

        // Abbreviate HOME directory
        if let Some(home_str) = ctx.home.to_str().map(|s| s.replace('\\', "/")) {
            if !home_str.is_empty() && dir_str.starts_with(&home_str) {
                dir_str = dir_str.replacen(&home_str, &self.home_symbol, 1);
            }
        }

        // Remove UNC prefix on Windows (\\?\)
        if dir_str.starts_with("//?/") {
            dir_str = dir_str[4..].to_string();
        }

        Some(PromptSegment::new(
            format!("{} ", dir_str),
            self.style.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::config::AshConfig;
    use std::path::PathBuf;

    #[test]
    fn test_directory_normal() {
        let ctx = AshContext::new(
            PathBuf::from("/home/user/projects/myapp"),
            PathBuf::from("/home/user"),
            None,
            None,
            AshConfig::default(),
        );
        let module = DirectoryModule::new(&AshConfig::default());
        let seg = module.render(&ctx).unwrap();
        assert_eq!(seg.content, "~/projects/myapp ");
    }

    #[test]
    fn test_directory_shows_full_path_no_truncation() {
        // Deep paths must render in full — no last-N-components truncation.
        let ctx = AshContext::new(
            PathBuf::from("/home/user/a/b/c/d"),
            PathBuf::from("/home/user"),
            None,
            None,
            AshConfig::default(),
        );
        let module = DirectoryModule::new(&AshConfig::default());
        let seg = module.render(&ctx).unwrap();
        assert_eq!(seg.content, "~/a/b/c/d ");
    }
}
