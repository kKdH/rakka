use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use rustc_hash::FxHashSet;
use tracing::{instrument, trace};

use triomphe::Arc;
use crate::behavior::Behavior;
use crate::mailbox::Mailbox;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct ActorId(u64);
impl ActorId {
    fn new() -> ActorId {
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        //NB: This assumes that u64 will not wrap around during a process' lifetime, which is a
        // safe assumption based on current technologies - it allows for 10^12 actors being spawned
        // per second for > 10000 years
        ActorId(COUNTER.fetch_add(1, Ordering::AcqRel))
    }
}

struct ActorRef<M: Send>(Arc<ActorRefInner<M>>);
impl <M: Send> ActorRef<M> {
    fn send(&self, msg: M) -> bool {
        self.send_envelope(Envelope::Message(msg))
    }

    fn send_envelope(&self, envelope: Envelope<M>) -> bool {
        self.0.message_sender.try_send(envelope).is_ok()
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
impl <M: Send> Clone for ActorRef<M> {
    fn clone(&self) -> Self {
        ActorRef(self.0.clone())
    }
}
impl <M: Send> Debug for ActorRef<M> {
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
    fn signal(&self, signal: Signal) -> bool {
        self.0.signal(signal)
    }
    fn stop(&self) -> bool {
        self.0.stop()
    }
}
impl <M: Send + 'static> From<ActorRef<M>> for GenericActorRef {
    fn from(value: ActorRef<M>) -> Self {
        GenericActorRef(Box::new(value))
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

struct ActorRefInner<M> {
    id: ActorId,
    message_sender: tokio::sync::mpsc::Sender<Envelope<M>>,
    system_sender: tokio::sync::mpsc::Sender<Signal>,
}


struct ActorCell<M: Send + 'static> {
    ctx: ActorContext<M>,
    mailbox: Mailbox<M>,

    behavior: Box<dyn Behavior<M> + Send>, //TODO is there a static representation?
    death_watchers: FxHashSet<GenericActorRef>,
}
impl <M: Send + Debug + 'static> ActorCell<M> {
    #[instrument]
    async fn message_loop(mut self) {
        trace!("starting message loop");
        while let Some(envelope) = self.mailbox.next().await {
            trace!("received {:?}", envelope);
            match &envelope {
                Envelope::Signal(Signal::Terminate) => {
                    trace!("terminating actor");
                    if self.death_watchers.len() > 0 {
                        trace!("notifying {} death watchers", self.death_watchers.len());
                        for dw in &self.death_watchers {
                            dw.signal(Signal::Death(self.ctx.myself.id()));
                        }
                    }
                    break;
                }
                Envelope::Signal(Signal::Watch { subscriber }) => {
                    self.death_watchers.insert(subscriber.clone());
                }
                _ => {}
            }

            //TODO handle panic!()
            if let Err(err) = self.behavior.receive(&mut self.ctx, envelope) {
                todo!()
            }
        }
        trace!("exiting message loop");
    }
}
impl <M: Send + 'static> Debug for ActorCell<M> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorCell({})", self.ctx.myself.0.id.0)
    }
}

enum SupervisorDecision {
    Resume,
    Restart,
    Stop,
    Escalate,
}
trait SupervisionStrategy {
    fn decide_on_failure(&self) -> SupervisorDecision;
}


#[derive(Debug)]
pub enum Signal {
    Death(ActorId),
    Watch { subscriber: GenericActorRef },
    //TODO unwatch?
    //TODO PostStop
    Terminate
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

struct ActorRuntime {
    tokio_handle: tokio::runtime::Handle,
}
fn spawn_actor<M: 'static + Debug + Send>(actor_runtime: &Arc<ActorRuntime>, behavior: impl Behavior<M> + 'static + Send, parent: Option<Box<dyn SignalSender>>) -> ActorRef<M> {
    let id = ActorId::new();
    trace!("spawning new actor {:?}", id); //TODO Debug for Behavior -> impl Into<Behavior<M>>
    let (message_sender, system_sender, mailbox) = Mailbox::new(128); //TODO mailbox size

    let actor_ref = ActorRef(Arc::new(ActorRefInner {
        id,
        message_sender,
        system_sender,
    }));

    let actor_cell = ActorCell {
        mailbox,
        ctx: ActorContext {
            myself: actor_ref.clone(),
            parent,
            children: Default::default(),
            inner: actor_runtime.clone(),
        },
        behavior: Box::new(behavior),
        death_watchers: Default::default(),
    };

    actor_runtime.tokio_handle.spawn(actor_cell.message_loop());

    actor_ref
}


pub struct ActorContext<M: Send + 'static> {
    myself: ActorRef<M>,
    parent: Option<Box<dyn SignalSender>>,
    children: FxHashSet<GenericActorRef>,
    inner: Arc<ActorRuntime>,
}
impl <M: Send + 'static> ActorContext<M> {
    fn spawn<N: 'static + Debug + Send>(&mut self, behavior: impl Behavior<N> + 'static + Send) -> ActorRef<N> {
        let result = spawn_actor(&self.inner, behavior, Some(Box::new(self.myself.clone())));
        self.children.insert(result.clone().into());
        result
    }
}


struct ActorSystem {
    inner: Arc<ActorRuntime>,
}
impl ActorSystem {
    fn new() -> (ActorSystem, impl Future<Output = ()> ) {
        let tokio_handle = tokio::runtime::Handle::try_current()
            .expect("An ActorSystem can only be created from the context of a Tokio runtime");

        let actor_system = ActorSystem {
            inner: Arc::new(ActorRuntime {
                tokio_handle
            })
        };
        (
            actor_system,
            tokio::time::sleep(Duration::from_secs(1)) //TODO lifecycle, shutdown
        )
    }

    fn spawn<M: 'static + Debug + Send>(&mut self, behavior: impl Behavior<M> + 'static + Send) -> ActorRef<M> { //TODO single top-level actor?
        spawn_actor(&self.inner, behavior, None) //TODO synthetic root actor per ActorSystem
    }
}

#[cfg(test)]
mod test {
    use tracing::{info, Level};
    use tracing_subscriber::FmtSubscriber;
    use super::*;

    #[ctor::ctor]
    fn init_tracing() {
        let subscriber = FmtSubscriber::builder()

            // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
            // will be written to stdout.
            .with_max_level(Level::TRACE)
            // completes the builder.
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed")
        ;
    }


    #[tokio::test]
    async fn test_simple() {
        fn dumping_behavior(_ctx: &mut ActorContext<String>, s: String) {
            info!("{}", s);
        }

        fn dw_behavior(ctx: &mut ActorContext<()>, msg: ()) {}

        let (mut actor_system, shutdown_handle) = ActorSystem::new();
        let actor_ref = actor_system.spawn(dumping_behavior);

        let dw_ref = actor_system.spawn(dw_behavior);
        actor_ref.signal(Signal::Watch { subscriber: dw_ref.into() });

        actor_ref.send("yo1".to_string());
        actor_ref.send("yo2".to_string());
        actor_ref.stop();
        actor_ref.send("yo3".to_string());

        shutdown_handle.await
    }
}


//TODO
// "no external ActorRefs"
// ActorRef -> Future
// supervision
// stop() method on context to stop child actors
// shutdown
// support for Span across message sends (?)
// stash / unstash
// become
// ReceiveTimeout
// aroundReceive
// DeathPact
// monitoring - mailbox size, delay, throughput, ...