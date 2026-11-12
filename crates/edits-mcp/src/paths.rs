//! Path confinement. Every path a tool receives is resolved against the
//! workspace root and refused when it ends up anywhere else, whether through
//! `..`, an absolute path or a symbolic link.

use std::path::{Component, Path, PathBuf};

/// The folder the server may read and write. Canonical, so `starts_with`
/// comparisons are meaningful.
#[derive(Clone, Debug)]
pub struct Workspace {
    root: PathBuf,
}

/// How many symbolic links one path may pass through before it is refused.
const MAX_LINKS: usize = 32;

impl Workspace {
    pub fn new(root: &Path) -> Result<Self, String> {
        let canonical = root
            .canonicalize()
            .map_err(|e| format!("the workspace root {} cannot be used: {e}. Start the server in an existing folder or set EDITS_ROOT to one.", root.display()))?;
        if !canonical.is_dir() {
            return Err(format!("the workspace root {} is not a folder; set EDITS_ROOT to a folder", canonical.display()));
        }
        Ok(Self { root: canonical })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a path argument (absolute, or relative to the root) to an
    /// absolute path inside the root. The path does not have to exist.
    pub fn resolve(&self, arg: &str, what: &str) -> Result<PathBuf, String> {
        self.resolve_from(&self.root, arg, what)
    }

    /// Like [`resolve`](Self::resolve), with relative paths taken from `base`
    /// (for paths written inside a project file, which are relative to it).
    pub fn resolve_from(&self, base: &Path, arg: &str, what: &str) -> Result<PathBuf, String> {
        if arg.trim().is_empty() {
            return Err(format!("{what}: the path is empty"));
        }
        if arg.contains('\0') {
            return Err(format!("{what}: the path contains a NUL byte"));
        }
        let given = Path::new(arg);
        let joined = if given.is_absolute() { given.to_path_buf() } else { base.join(given) };
        let resolved = follow(&joined).map_err(|e| format!("{what}: cannot resolve \"{arg}\": {e}"))?;
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "{what}: \"{arg}\" is outside the workspace root {} (it resolves to {}). Use a path inside the root, copy the file into it, or restart the server with EDITS_ROOT set to a folder that contains both.",
                self.root.display(),
                resolved.display()
            ));
        }
        Ok(resolved)
    }

    /// Resolve a path that must be an existing file.
    pub fn file(&self, arg: &str, what: &str) -> Result<PathBuf, String> {
        let path = self.resolve(arg, what)?;
        if path.is_file() {
            Ok(path)
        } else if path.is_dir() {
            Err(format!("{what}: {} is a folder, not a file", self.show(&path)))
        } else {
            Err(format!("{what}: {} does not exist (paths are relative to the workspace root {})", self.show(&path), self.root.display()))
        }
    }

    /// The path as it is best written in a message: relative to the root.
    pub fn show(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
            Ok(rel) => rel.display().to_string(),
            Err(_) => path.display().to_string(),
        }
    }
}

/// Resolve `.`, `..` and symbolic links the way the kernel would, without
/// requiring the last components to exist. A link that points nowhere resolves
/// to its target, because writing through it would create a file there.
fn follow(path: &Path) -> std::io::Result<PathBuf> {
    // Components still to walk, last one first, owned so that the targets of
    // links can be pushed onto the same stack.
    let mut pending: Vec<PathBuf> = path.components().rev().map(|c| PathBuf::from(c.as_os_str())).collect();
    let mut out = PathBuf::new();
    let mut links = 0;
    while let Some(part) = pending.pop() {
        match part.components().next() {
            None | Some(Component::CurDir) => {}
            Some(c @ (Component::Prefix(_) | Component::RootDir)) => out.push(c.as_os_str()),
            Some(Component::ParentDir) => {
                out.pop();
            }
            Some(Component::Normal(name)) => {
                let next = out.join(name);
                let is_link = std::fs::symlink_metadata(&next).map(|m| m.file_type().is_symlink()).unwrap_or(false);
                if !is_link {
                    out = next;
                    continue;
                }
                links += 1;
                if links > MAX_LINKS {
                    return Err(std::io::Error::other("too many symbolic links"));
                }
                let target = std::fs::read_link(&next)?;
                if target.is_absolute() {
                    out = PathBuf::new();
                }
                pending.extend(target.components().rev().map(|c| PathBuf::from(c.as_os_str())));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("edits-mcp-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("root/sub")).unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::fs::write(dir.join("root/sub/p.json"), "{}").unwrap();
        std::fs::write(dir.join("outside/secret.txt"), "s").unwrap();
        dir
    }

    #[test]
    fn inside_paths_resolve_whether_or_not_they_exist() {
        let dir = temp("inside");
        let ws = Workspace::new(&dir.join("root")).unwrap();
        let root = ws.root().to_path_buf();
        assert_eq!(ws.resolve("sub/p.json", "project").unwrap(), root.join("sub/p.json"));
        assert_eq!(ws.resolve("./sub/../sub/p.json", "project").unwrap(), root.join("sub/p.json"));
        assert_eq!(ws.resolve("new/dir/out.mp4", "output").unwrap(), root.join("new/dir/out.mp4"));
        assert_eq!(ws.resolve(root.join("sub").to_str().unwrap(), "dir").unwrap(), root.join("sub"));
        assert_eq!(ws.resolve(".", "dir").unwrap(), root);
        assert_eq!(ws.show(&root.join("sub/p.json")), "sub/p.json");
        assert!(ws.file("sub/p.json", "project").is_ok());
        assert!(ws.file("sub/missing.json", "project").unwrap_err().contains("does not exist"));
        assert!(ws.file("sub", "project").unwrap_err().contains("is a folder"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn outside_paths_are_refused() {
        let dir = temp("outside");
        let ws = Workspace::new(&dir.join("root")).unwrap();
        for bad in ["../outside/secret.txt", "sub/../../outside/secret.txt", "/etc/passwd", "../root-sibling/x", ".."] {
            let e = ws.resolve(bad, "project").unwrap_err();
            assert!(e.contains("outside the workspace root"), "{bad}: {e}");
            assert!(e.contains("EDITS_ROOT"), "the error says what to do: {e}");
        }
        let abs = dir.join("outside/secret.txt");
        assert!(ws.resolve(abs.to_str().unwrap(), "video").is_err());
        assert!(ws.resolve("", "project").unwrap_err().contains("empty"));
        // relative to a base inside the root: a slot path that climbs out
        let base = ws.root().join("sub");
        assert!(ws.resolve_from(&base, "../ok.png", "slot").is_ok());
        assert!(ws.resolve_from(&base, "../../outside/secret.txt", "slot").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_leave_the_root() {
        use std::os::unix::fs::symlink;
        let dir = temp("links");
        let root = dir.join("root");
        symlink(dir.join("outside"), root.join("escape")).unwrap();
        symlink(dir.join("outside/secret.txt"), root.join("file-link")).unwrap();
        symlink(dir.join("outside/not-yet.mp4"), root.join("dangling")).unwrap();
        symlink("sub", root.join("inner")).unwrap();
        let ws = Workspace::new(&root).unwrap();
        assert!(ws.resolve("escape/secret.txt", "video").unwrap_err().contains("outside"));
        assert!(ws.resolve("escape/new.mp4", "output").unwrap_err().contains("outside"));
        assert!(ws.resolve("file-link", "video").unwrap_err().contains("outside"));
        // writing through a dangling link would create the file outside
        assert!(ws.resolve("dangling", "output").unwrap_err().contains("outside"));
        // a link that stays inside is fine and resolves to its target
        assert_eq!(ws.resolve("inner/p.json", "project").unwrap(), ws.root().join("sub/p.json"));
        // the kernel resolves `link/..` through the link's target, and so do we
        assert!(ws.resolve("escape/../root/sub/p.json", "project").is_ok());
        assert!(ws.resolve("escape/../outside/secret.txt", "project").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
