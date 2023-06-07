use std::any::Any;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use rustc_hash::FxHashSet;
use tracing::{debug, error, instrument, trace, warn};

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

struct ActorRef<M: Send + 'static>(Arc<ActorRefInner<M>>);
impl <M: Send + 'static> ActorRef<M> {
    fn send(&self, msg: M) -> bool {
        self.send_envelope(Envelope::Message(msg))
    }

    fn send_envelope(&self, envelope: Envelope<M>) -> bool {
        self.0.message_sender.try_send(envelope).is_ok()
    }

    fn as_generic(&self) -> GenericActorRef {
        GenericActorRef(Box::new(self.clone()))
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
    fn signal(&self, signal: Signal) -> bool {
        self.0.signal(signal)
    }
    fn stop(&self) -> bool {
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

struct ActorRefInner<M> {
    id: ActorId,
    message_sender: tokio::sync::mpsc::Sender<Envelope<M>>,
    system_sender: tokio::sync::mpsc::Sender<Signal>,
}


struct ActorCell<M: Send + 'static> {
    ctx: ActorContext<M>,
    mailbox: Mailbox<M>,
    is_terminating: bool,
    suspend_count: u32,

    behavior: Box<dyn Behavior<M> + Send>, //TODO is there a static representation?
    supervision_strategy: Box<dyn SupervisionStrategy>,
    death_watchers: FxHashSet<GenericActorRef>,
}
impl <M: Send + Debug + 'static> ActorCell<M> {
    #[instrument]
    async fn message_loop(mut self) {
        trace!("starting message loop");
        while let Some(envelope) = self.mailbox.next(self.suspend_count > 0).await {
            trace!("received {:?}", envelope);
            match &envelope {
                Envelope::Signal(Signal::ChildTerminated(child_ref)) => self.handle_child_terminated(child_ref),
                Envelope::Signal(Signal::Suspend) => self.handle_suspend(),
                Envelope::Signal(Signal::Terminate) => self.handle_terminate(),
                Envelope::Signal(Signal::Watch { subscriber }) => {
                    self.death_watchers.insert(subscriber.clone());
                }
                _ => {}
            }

            if self.is_terminating && self.ctx.children.is_empty() {
                trace!("actually terminating actor");

                if let Some(parent) = self.ctx.parent {
                    parent.signal(Signal::ChildTerminated(self.ctx.myself.as_generic()));
                }

                if self.death_watchers.len() > 0 {
                    trace!("notifying {} death watchers", self.death_watchers.len());
                    for dw in &self.death_watchers {
                        dw.signal(Signal::Death(self.ctx.myself.id()));
                    }
                }
                break;
            }

            //TODO handle panic!()

            let received = catch_unwind(AssertUnwindSafe(|| self.behavior.receive(&mut self.ctx, envelope)));
            let cause = match received {
                Err(e) => {
                    // panicked -> actor crashed
                    debug!("actor crashed by panicking");
                    CrashCause::Panic(e)
                }
                Ok(Err(e)) => {
                    // returned Err -> actor crashed
                    debug!("actor crashed returning an error: {}", e);
                    CrashCause::Err(e)
                }
                Ok(_) => {
                    continue;
                }
            };

            // this actor crashed - suspend it and ask supervisor
            self.ctx.myself.signal(Signal::Suspend);

            if let Some(supervisor) = &self.ctx.parent {
                supervisor.signal(Signal::SupervisionRequired(self.ctx.myself.as_generic(), cause, CrashLifecycleStage::ReceiveLoop));
            }
            else {
                todo!()
            }
        }
        trace!("exiting message loop");
    }

    fn handle_child_terminated(&mut self, child_ref: &GenericActorRef) {
        trace!("child {:?} terminated", child_ref);
        if !self.ctx.children.remove(child_ref) {
            debug!("terminated 'child' was not registered as a child");
        }
    }

    fn handle_suspend(&mut self) {
        trace!("suspending actor");
        if self.suspend_count == u32::MAX {
            warn!("max number of nested suspends exceeded"); //TODO
        }
        self.suspend_count += 1;
        for child in &self.ctx.children {
            child.signal(Signal::Suspend);
        }
    }

    fn handle_terminate(&mut self) {
        trace!("start terminating actor");
        self.is_terminating = true;

        for child in &self.ctx.children {
            child.signal(Signal::Terminate);
        }
    }

    //TODO lifecycle callbacks

    fn handle_restart(&mut self) {
        if self.is_terminating {
            trace!("skipping actor restart because it is already terminating");
        }
        else {
            trace!("restarting actor");
            if self.suspend_count == 0 {
                error!("inconsistent internal actor state - restarting though not suspended"); //TODO
            }
            self.suspend_count -= 1;
            //TODO replace behavior with new behavior
        }
    }

    fn handle_supervision_required(&mut self, crashed_actor: GenericActorRef, cause: CrashCause, lifecycle_stage: CrashLifecycleStage) {
        match self.supervision_strategy.decide_on_failure(&cause, &lifecycle_stage) {
            SupervisorDecision::Restart => { crashed_actor.signal(Signal::Restart); }, //TODO handle 'signal()' return value
            SupervisorDecision::Stop => { crashed_actor.signal(Signal::Terminate); },
            SupervisorDecision::Escalate => {
                self.suspend_count += 1;
                for child in &self.ctx.children {
                    if child != &crashed_actor {
                        child.signal(Signal::Suspend);
                    }
                }
                if let Some(supervisor) = &self.ctx.parent {
                    supervisor.signal(Signal::SupervisionRequired(self.ctx.myself.as_generic(), cause, lifecycle_stage));
                }
                else {
                    todo!();
                }
            }
        }
    }
}
impl <M: Send + 'static> Debug for ActorCell<M> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorCell({})", self.ctx.myself.0.id.0)
    }
}

#[derive(Debug)]
enum CrashCause {
    Panic(Box<dyn Any+Send>),
    Err(anyhow::Error),
}

#[derive(Debug)]
enum CrashLifecycleStage {
    ReceiveLoop,
}

enum SupervisorDecision {
    Restart,
    Stop,
    Escalate,
}
trait SupervisionStrategy: Send {
    fn decide_on_failure(&self, cause: &CrashCause, crash_lifecycle_stage: &CrashLifecycleStage) -> SupervisorDecision;
}

struct StoppingSupervisionStrategy {}
impl SupervisionStrategy for StoppingSupervisionStrategy {
    fn decide_on_failure(&self, _cause: &CrashCause, crash_lifecycle_stage: &CrashLifecycleStage) -> SupervisorDecision {
        SupervisorDecision::Stop
    }
}
struct RestartingSupervisionStrategy {}
impl SupervisionStrategy for RestartingSupervisionStrategy {
    fn decide_on_failure(&self, _cause: &CrashCause, crash_lifecycle_stage: &CrashLifecycleStage) -> SupervisorDecision {
        SupervisorDecision::Restart
    }
}


#[derive(Debug)]
pub enum Signal {
    ChildTerminated(GenericActorRef),
    Death(ActorId),
    Watch { subscriber: GenericActorRef },
    //TODO unwatch?
    //TODO PostStop
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

struct ActorRuntime {
    tokio_handle: tokio::runtime::Handle,
}
fn spawn_actor<M: 'static + Debug + Send>(actor_runtime: &Arc<ActorRuntime>, behavior: impl Behavior<M> + 'static + Send, parent: Option<GenericActorRef>) -> ActorRef<M> {
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
        is_terminating: false,
        suspend_count: 0,
        behavior: Box::new(behavior),
        supervision_strategy: Box::new(StoppingSupervisionStrategy{}),
        death_watchers: Default::default(),
    };

    actor_runtime.tokio_handle.spawn(actor_cell.message_loop());

    actor_ref
}


pub struct ActorContext<M: Send + 'static> {
    myself: ActorRef<M>,
    parent: Option<GenericActorRef>,
    children: FxHashSet<GenericActorRef>,
    inner: Arc<ActorRuntime>,
}
impl <M: Send + 'static> ActorContext<M> {
    fn spawn<N: 'static + Debug + Send>(&mut self, behavior: impl Behavior<N> + 'static + Send) -> ActorRef<N> {
        let result = spawn_actor(&self.inner, behavior, Some(self.myself.as_generic()));
        self.children.insert(result.as_generic());
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
        actor_ref.signal(Signal::Watch { subscriber: dw_ref.as_generic() });

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
// stop() method on context to stop child actors
// shutdown
// support for Span across message sends (?)
// stash / unstash
// become
// ReceiveTimeout
// aroundReceive
// DeathPact
// monitoring - mailbox size, delay, throughput, ...
