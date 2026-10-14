//! Not vanilla: `/backup` and `/rollback`, the operator's side of backup.rs.

use super::{usage, Ctx, Effect, Line, R};

pub(super) const BACKUP: &str = "/backup [list]";
pub(super) const ROLLBACK: &str = "/rollback [list|cancel|<backup name>]";
/// The backups a list names, newest first.
const LISTED: usize = 10;

impl Ctx<'_> {
    /// `/backup` saves and copies the world now (the backup thread answers when it is done);
    /// `/backup list` names the newest backups.
    pub(super) fn backup(&mut self, args: &[&str]) -> R {
        match args {
            [] => {
                self.message(Line::plain("Saving a backup of the world..."));
                self.effects.push(Effect::Backup);
                Ok(())
            }
            [a] if a.eq_ignore_ascii_case("list") => {
                self.list_backups();
                Ok(())
            }
            _ => Err(usage(BACKUP)),
        }
    }

    fn list_backups(&mut self) {
        let all = crate::backup::list(self.world.dir());
        self.message(Line::plain(format!("There are {} backups:", all.len())));
        if !all.is_empty() {
            let more = if all.len() > LISTED { format!(" and {} older", all.len() - LISTED) } else { String::new() };
            self.message(Line::plain(format!("{}{more}", all[..all.len().min(LISTED)].join(", "))));
        }
        if let Some(p) = crate::backup::pending(self.world.dir()) {
            self.message(Line::plain(format!("The world rolls back to {p} when the server next starts")));
        }
    }

    /// `/rollback NAME` puts the world back as that backup left it at the next start (the world
    /// as it is gets a backup of its own first). `/rollback cancel` takes the request back;
    /// `/rollback` or `/rollback list` names the backups.
    pub(super) fn rollback(&mut self, args: &[&str]) -> R {
        match args {
            [] => self.list_backups(),
            [a] if a.eq_ignore_ascii_case("list") => self.list_backups(),
            [a] if a.eq_ignore_ascii_case("cancel") => {
                if crate::backup::pending(self.world.dir()).is_none() {
                    return Err("No rollback is waiting".into());
                }
                self.effects.push(Effect::Rollback(None));
                self.message(Line::plain("The rollback is cancelled"));
            }
            [name] => {
                if !crate::backup::list(self.world.dir()).iter().any(|b| b == name) {
                    return Err(format!("There is no backup named {name}; /rollback list names them"));
                }
                self.effects.push(Effect::Rollback(Some(name.to_string())));
                self.message(Line::plain(format!(
                    "The world rolls back to {name} when the server next starts. Restart the server to roll back; the world as it is now is backed up first"
                )));
            }
            _ => return Err(usage(ROLLBACK)),
        }
        Ok(())
    }
}
