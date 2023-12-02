use std::fmt::Debug;
use rustc_hash::FxHashSet;
use triomphe::Arc;
use crate::actor::{ActorRuntime, spawn_actor};
use crate::behavior::ActorBehavior;
use crate::refs::{ActorRef, GenericActorRef};

pub struct ActorContext<M: Send + 'static> {
    pub(crate) myself: ActorRef<M>,
    pub(crate) parent: Option<GenericActorRef>,
    pub(crate) children: FxHashSet<GenericActorRef>,
    pub(crate) inner: Arc<ActorRuntime>,
    pub(crate)death_watchers: FxHashSet<GenericActorRef>,
}
impl <M: Send + 'static> ActorContext<M> {
    pub fn spawn<N: 'static + Debug + Send>(&mut self, behavior: impl ActorBehavior<N> + 'static + Send) -> ActorRef<N> {
        let result = spawn_actor(&self.inner, behavior, Some(self.myself.as_generic()));
        self.children.insert(result.as_generic());
        result
    }

    pub fn children(&self) -> impl Iterator<Item = &GenericActorRef> + '_ {
        self.children.iter()
    }

    pub fn myself(&self) -> ActorRef<M> {
        self.myself.clone()
    }
}
