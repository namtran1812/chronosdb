use super::{Snapshot, TransactionId, TransactionState, TupleVersion};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VersionChainError {
    #[error("no visible tuple version exists")]
    NoVisibleVersion,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionChain {
    versions: Vec<TupleVersion>,
}

impl VersionChain {
    pub fn new() -> Self {
        Self {
            versions: Vec::new(),
        }
    }

    pub fn versions(&self) -> &[TupleVersion] {
        &self.versions
    }

    pub fn insert(&mut self, writer: TransactionId, payload: Vec<u8>) {
        self.versions.push(TupleVersion::new(writer, payload));
    }

    pub fn visible_version<'a, F>(
        &'a self,
        snapshot: &Snapshot,
        reader: TransactionId,
        mut state_of: F,
    ) -> Option<&'a TupleVersion>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        self.versions
            .iter()
            .rev()
            .find(|version| version.visible_to(snapshot, reader, &mut state_of))
    }

    pub fn update<F>(
        &mut self,
        snapshot: &Snapshot,
        writer: TransactionId,
        payload: Vec<u8>,
        mut state_of: F,
    ) -> Result<(), VersionChainError>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let index = self
            .visible_index(snapshot, writer, &mut state_of)
            .ok_or(VersionChainError::NoVisibleVersion)?;

        self.versions[index].mark_deleted(writer);

        self.versions.push(TupleVersion::new(writer, payload));

        Ok(())
    }

    pub fn delete<F>(
        &mut self,
        snapshot: &Snapshot,
        writer: TransactionId,
        mut state_of: F,
    ) -> Result<(), VersionChainError>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        let index = self
            .visible_index(snapshot, writer, &mut state_of)
            .ok_or(VersionChainError::NoVisibleVersion)?;

        self.versions[index].mark_deleted(writer);

        Ok(())
    }

    fn visible_index<F>(
        &self,
        snapshot: &Snapshot,
        reader: TransactionId,
        mut state_of: F,
    ) -> Option<usize>
    where
        F: FnMut(TransactionId) -> Option<TransactionState>,
    {
        self.versions
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, version)| {
                version
                    .visible_to(snapshot, reader, &mut state_of)
                    .then_some(index)
            })
    }
}
