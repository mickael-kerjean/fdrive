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
            Plan::Remove { path, dir } => self.replay_remove(path, *dir).await,
        }
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

    async fn replay_remove(&self, path: &RelPath, dir: bool) -> Outcome {
        if self.is_frozen(path) {
            return Outcome::Busy;
        }
        let target = if dir { path.as_dir() } else { path.as_file() };
        match self.sdk.rm(&target).await {
            Ok(()) | Err(SdkError::NotFound) => Outcome::Removed,
            // some backends answer rm of a missing target with a generic
            // error instead of 404; already-gone is still success
            Err(err) => {
                if self.gone(path, dir).await {
                    log::debug!("rm {target} failed ({err}) but the target is gone");
                    Outcome::Removed
                } else {
                    Outcome::Failed(err.into())
                }
            }
        }
    }

    async fn gone(&self, path: &RelPath, dir: bool) -> bool {
        if dir {
            matches!(self.sdk.ls(&path.as_dir()).await, Err(SdkError::NotFound))
        } else {
            matches!(
                self.sdk.stat(&path.as_file()).await,
                Err(SdkError::NotFound)
            )
        }
    }
}
