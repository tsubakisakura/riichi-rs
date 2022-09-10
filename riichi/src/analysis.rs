pub mod decomp;
pub mod irregular;

use std::fmt::{Display, Formatter};

use itertools::Itertools;

use crate::{
    common::*,
};
pub use self::{
    decomp::{Decomposer, RegularWait},
    irregular::{IrregularWait, detect_irregular_wait},
};

#[derive(Copy, Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]  // due to `RegularWait`
pub enum Wait {
    Regular(RegularWait),
    Irregular(IrregularWait),
}

// TODO(summivox): better name
#[derive(Clone, Debug, Default)]
pub struct WaitingInfo {
    pub waiting_set: TileMask34,
    pub regular: Vec<RegularWait>,
    pub irregular: Option<IrregularWait>,
}

impl WaitingInfo {
    pub fn from_keys(decomposer: &mut Decomposer, keys: &[u32; 4]) -> Self {
        let mut waiting_set = TileMask34::default();
        let regular = decomposer.with_keys(*keys).iter().collect_vec();
        for wait in regular.iter() {
            waiting_set.0 |= 1 << wait.waiting_tile.encoding() as u64;
        }
        let irregular = detect_irregular_wait(*keys);
        if let Some(irregular) = irregular {
            waiting_set |= irregular.to_waiting_set();
        }
        Self { waiting_set, regular, irregular }
    }
}

impl Display for WaitingInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{waiting_set={}", self.waiting_set)?;
        if let Some(irregular) = self.irregular {
            write!(f, " irregular={}", irregular)?;
        }
        write!(f, " regular=[")?;
        for w in &self.regular {
            write!(f, "({}),", w)?;
        }
        write!(f, "]}}")
    }
}
