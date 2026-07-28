use std::io;

use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::Error as SdkError;

use super::Engine;
use crate::model::{Conflict, Observation, Operation, Plan};

pub(super) enum Outcome {
    Saved {
        obs: Option<Observation>,
        sig: Option<Vec<u8>>,
        reedited: bool,
    },
    Diverted {
        theirs: Option<Observation>,
        copy: RelPath,
        obs: Option<Observation>,
        sig: Option<Vec<u8>>,
        conflict: Conflict,
    },
    Moved,
    MoveLost {
        theirs: Option<Observation>,
        resurrect: Option<RelPath>,
        conflict: Option<Conflict>,
    },
    Removed,
    RemoveLost {
        theirs: Observation,
        conflict: Conflict,
    },
    RemoveDirLost {
        conflict: Conflict,
    },
    Busy,
    Failed(io::Error),
}

impl<T: LocalStore> Engine<T> {
    pub(super) async fn replay(&self, plan: &Plan) -> Outcome {
        match plan {
            Plan::Save {
                path,
                replaces,
                reuses,
            } => self.replay_save(path, *replaces, reuses.as_ref()).await,
            Plan::Move { from, to, moves } => self.replay_move(from, to, *moves).await,
            Plan::Remove { path, removes } => self.replay_remove(path, *removes).await,
            Plan::RemoveDir { path } => self.replay_remove_dir(path).await,
        }
    }

    async fn replay_remove_dir(&self, path: &RelPath) -> Outcome {
        if self.is_frozen(path) || self.state().busy_under(path) {
            return Outcome::Busy;
        }
        match self.subtree_holds_files(path).await {
            Err(SdkError::NotFound) => Outcome::Removed,
            Err(err) => Outcome::Failed(err.into()),
            Ok(true) => Outcome::RemoveDirLost {
                conflict: Conflict::new(Operation::Delete(path.clone()), None, None, None),
            },
            Ok(false) => match self.sdk.rm(&path.as_dir()).await {
                Ok(()) | Err(SdkError::NotFound) => Outcome::Removed,
                Err(err) => Outcome::Failed(err.into()),
            },
        }
    }

    async fn subtree_holds_files(&self, path: &RelPath) -> Result<bool, SdkError> {
        let mut stack = vec![path.clone()];
        while let Some(dir) = stack.pop() {
            let listing = match self.sdk.ls(&dir.as_dir()).await {
                Ok(listing) => listing,
                Err(SdkError::NotFound) if dir != *path => continue,
                Err(err) => return Err(err),
            };
            for entry in listing {
                match entry.kind {
                    crate::sdk::FileType::File => return Ok(true),
                    crate::sdk::FileType::Directory => stack.push(dir.join(&entry.name)),
                }
            }
        }
        Ok(false)
    }

    async fn replay_move(&self, from: &RelPath, to: &RelPath, moves: Observation) -> Outcome {
        if self.is_frozen(from) || self.is_frozen(to) {
            return Outcome::Busy;
        }
        match self.sdk.stat(&from.as_file()).await {
            Ok(info) if Observation::of(&info) == moves => {
                match self.sdk.mv(&from.as_file(), &to.as_file()).await {
                    Ok(()) => Outcome::Moved,
                    Err(SdkError::NotFound) => self.move_lost(from, to, moves, None),
                    Err(err) => Outcome::Failed(err.into()),
                }
            }
            Ok(info) => self.move_lost(from, to, moves, Some(Observation::of(&info))),
            Err(SdkError::NotFound) => self.move_lost(from, to, moves, None),
            Err(err) => Outcome::Failed(err.into()),
        }
    }

    fn move_lost(
        &self,
        from: &RelPath,
        to: &RelPath,
        moves: Observation,
        theirs: Option<Observation>,
    ) -> Outcome {
        let resurrect = self.local.backing(to).is_file().then(|| to.clone());
        let lost = theirs.is_some() || resurrect.is_none();
        Outcome::MoveLost {
            theirs,
            resurrect,
            conflict: lost.then(|| {
                Conflict::new(
                    Operation::Rename(from.clone(), to.clone()),
                    theirs.map(|_| moves),
                    theirs,
                    None,
                )
            }),
        }
    }

    async fn replay_remove(&self, path: &RelPath, removes: Observation) -> Outcome {
        if self.is_frozen(path) {
            return Outcome::Busy;
        }
        match self.sdk.stat(&path.as_file()).await {
            Err(SdkError::NotFound) => Outcome::Removed,
            Ok(info) if Observation::of(&info) == removes => {
                match self.sdk.rm(&path.as_file()).await {
                    Ok(()) | Err(SdkError::NotFound) => Outcome::Removed,
                    Err(err) => Outcome::Failed(err.into()),
                }
            }
            Ok(info) => {
                let theirs = Observation::of(&info);
                Outcome::RemoveLost {
                    theirs,
                    conflict: Conflict::new(
                        Operation::Delete(path.clone()),
                        Some(removes),
                        Some(theirs),
                        None,
                    ),
                }
            }
            Err(err) => Outcome::Failed(err.into()),
        }
    }
}
