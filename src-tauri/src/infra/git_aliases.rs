//! Git aliases, for reading a command line the way git will run it (F-2.5b):
//! `git pf` may be `push --force`, and `git st` may be `status`.
//!
//! The workspace repository's configuration, which includes the global and
//! system files; outside a repository, those alone. Anything unreadable is
//! no aliases — an unknown `git` subcommand then asks, the safe reading.

use std::collections::HashMap;
use std::path::Path;

use git2::{Config, Repository};

use crate::domain::command_risk::GitAliases;

pub fn read(workspace: &Path) -> GitAliases {
    let config = match Repository::discover(workspace) {
        Ok(repo) => repo.config(),
        Err(_) => Config::open_default(),
    };
    let mut aliases = HashMap::new();
    let Ok(config) = config else { return aliases };
    let Ok(entries) = config.entries(Some("alias\\..*")) else { return aliases };
    let _ = entries.for_each(|entry| {
        if let (Ok(name), Ok(value)) = (entry.name(), entry.value()) {
            // Git stores the key lower-cased; the name after `alias.` is
            // what is typed after `git`.
            if let Some(alias) = name.strip_prefix("alias.") {
                aliases.insert(alias.to_string(), value.to_string());
            }
        }
    });
    aliases
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn the_repository_s_aliases_are_read() {
        let dir = temp_dir("git-aliases");
        let repo = Repository::init(&dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("alias.pf", "push --force").unwrap();
        config.set_str("alias.st", "status -sb").unwrap();

        let aliases = read(&dir);
        assert_eq!(aliases.get("pf").map(String::as_str), Some("push --force"));
        assert_eq!(aliases.get("st").map(String::as_str), Some("status -sb"));
    }
}
