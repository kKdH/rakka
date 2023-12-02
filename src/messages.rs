use crate::refs::{ActorId, GenericActorRef};
use crate::supervision::{CrashCause, CrashLifecycleStage};

#[derive(Debug)]
pub enum Signal {
    ChildTerminated(GenericActorRef),
    Death(ActorId),
    RegisterDeathWatcher { subscriber: GenericActorRef },
    UnregisterDeathWatcher { subscriber: GenericActorRef },
    Suspend,
    Terminate,
    Restart,

    SupervisionRequired(GenericActorRef, CrashCause, CrashLifecycleStage),
}
impl <M> Into<Envelope<M>> for Signal {
    fn into(self) -> Envelope<M> {
        Envelope::Signal(self)
    }
}

#[derive(Debug)]
pub enum Envelope<M> {
    Message(M),
    Signal(Signal),
}
