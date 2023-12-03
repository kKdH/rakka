use std::fmt::Debug;
use std::future::Future;
use std::time::Duration;
use tracing::trace;

use triomphe::Arc;

use crate::behavior::ActorBehavior;
use crate::cell::Actor;
use crate::context::ActorContext;
use crate::mailbox::Mailbox;
use crate::refs::{ActorId, ActorRef, ActorRefInner, GenericActorRef, SignalSender};
use crate::supervision::StoppingSupervisionStrategy;


pub struct ActorRuntime {
    tokio_handle: tokio::runtime::Handle,
}
pub(crate) fn spawn_actor<M: 'static + Debug + Send>(actor_runtime: &Arc<ActorRuntime>, behavior: impl ActorBehavior<M> + 'static + Send + Clone, parent: Option<GenericActorRef>) -> ActorRef<M> {
    let id = ActorId::new();
    trace!("spawning new actor {:?}", id); //TODO Debug for Behavior -> impl Into<Behavior<M>>
    let (message_sender, system_sender, mailbox) = Mailbox::new(128); //TODO mailbox size

    let actor_ref = ActorRef(Arc::new(ActorRefInner {
        id,
        message_sender,
        system_sender,
    }));

    let initial_behavior = Box::new(behavior.clone());

    let actor_cell = Actor {
        mailbox,
        ctx: ActorContext {
            myself: actor_ref.clone(),
            parent,
            children: Default::default(),
            inner: actor_runtime.clone(),
            death_watchers: Default::default(),
        },
        is_terminating: false,
        suspend_count: 0,
        initial_behavior: Box::new(move || initial_behavior.clone()),
        behavior: Box::new(behavior),
        supervision_strategy: StoppingSupervisionStrategy{},
    };

    actor_runtime.tokio_handle.spawn(actor_cell.message_loop());

    actor_ref
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

    fn spawn<M: 'static + Debug + Send>(&mut self, behavior: impl ActorBehavior<M> + 'static + Send + Clone) -> ActorRef<M> { //TODO single top-level actor?
        spawn_actor(&self.inner, behavior, None) //TODO synthetic root actor per ActorSystem
    }
}

#[cfg(test)]
mod test {
    use tracing::{info, Level};
    use tracing_subscriber::FmtSubscriber;
    use crate::messages::Signal;

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

        fn dw_behavior(_ctx: &mut ActorContext<()>, _msg: ()) {}

        let (mut actor_system, shutdown_handle) = ActorSystem::new();
        let actor_ref = actor_system.spawn(dumping_behavior);

        let dw_ref = actor_system.spawn(dw_behavior);
        actor_ref.signal(Signal::RegisterDeathWatcher { subscriber: dw_ref.as_generic() });

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
