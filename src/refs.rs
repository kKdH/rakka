use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use triomphe::Arc;

use crate::messages::{Envelope, Signal};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ActorId(u64);
impl ActorId {
    pub(crate) fn new() -> ActorId {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        //NB: This assumes that u64 will not wrap around during a process' lifetime, which is a
        // safe assumption based on current technologies - it allows for 10^12 actors being spawned
        // per second for > 10000 years
        ActorId(COUNTER.fetch_add(1, Ordering::AcqRel))
    }
}
impl Display for ActorId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct ActorRef<M: Send + 'static>(pub(crate) Arc<ActorRefInner<M>>);
impl <M: Send + 'static> ActorRef<M> {
    pub fn send(&self, msg: M) -> bool {
        self.send_envelope(Envelope::Message(msg))
    }

    pub fn send_envelope(&self, envelope: Envelope<M>) -> bool {
        self.0.message_sender.try_send(envelope).is_ok()
    }

    pub fn as_generic(&self) -> GenericActorRef {
        GenericActorRef(Box::new(self.clone()))
    }

    pub fn id(&self) -> ActorId {
        self.0.id
    }
}
impl <M: Send + 'static> SignalSender for ActorRef<M> {
    fn id(&self) -> ActorId {
        self.0.id
    }

    fn signal(&self, signal: Signal) -> bool {
        self.send_envelope(Envelope::Signal(signal))
    }

    fn stop(&self) -> bool {
        self.signal(Signal::Terminate)
    }

    fn clone_to_box(&self) -> Box<dyn SignalSender> {
        Box::new(self.clone())
    }
}
impl <M: Send + 'static> Clone for ActorRef<M> {
    fn clone(&self) -> Self {
        ActorRef(self.0.clone())
    }
}
impl <M: Send + 'static> Debug for ActorRef<M> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorRef({})", self.0.id.0)
    }
}

pub trait SignalSender: Debug + Send {
    fn id(&self) -> ActorId;

    fn signal(&self, signal: Signal) -> bool;
    fn stop(&self) -> bool;

    fn clone_to_box(&self) -> Box<dyn SignalSender>; //TODO move to separate trait to reduce visibility
}

#[derive(Debug)]
pub struct GenericActorRef(Box<dyn SignalSender>);
impl GenericActorRef {
    pub fn signal(&self, signal: Signal) -> bool {
        self.0.signal(signal)
    }
    pub fn stop(&self) -> bool {
        self.0.stop()
    }
}

impl Clone for GenericActorRef {
    fn clone(&self) -> Self {
        GenericActorRef(self.0.clone_to_box())
    }
}
impl Eq for GenericActorRef {}
impl PartialEq for GenericActorRef {
    fn eq(&self, other: &Self) -> bool {
        self.0.id() == other.0.id()
    }
}
impl Hash for GenericActorRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id().hash(state)
    }
}

pub(crate) struct ActorRefInner<M> {
    pub(crate) id: ActorId,
    pub(crate) message_sender: tokio::sync::mpsc::Sender<Envelope<M>>,
    pub(crate) system_sender: tokio::sync::mpsc::Sender<Signal>,
}

