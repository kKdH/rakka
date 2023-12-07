use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
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


pub struct ActorSystem {
    inner: Arc<ActorRuntime>,
    is_shutting_down: AtomicBool, //TODO state machine, coordinated shutdown
    shut_down_completed: Arc<Notify>,
}
impl ActorSystem {
    pub fn new() -> ActorSystem {
        //TODO register CTRL_C signal handler - offer 'await'? always / configurable SIGINT handling? separate await'ing function?

        let tokio_handle = tokio::runtime::Handle::try_current()
            .expect("An ActorSystem can only be created from the context of a Tokio runtime");

        let shut_down_notifier = Arc::new(Notify::new());

        let actor_system = ActorSystem {
            inner: Arc::new(ActorRuntime {
                tokio_handle
            }),
            is_shutting_down: AtomicBool::new(false),
            shut_down_completed: shut_down_notifier.clone(),
        };

        actor_system
    }

    pub fn spawn<M: 'static + Debug + Send>(&self, behavior: impl ActorBehavior<M> + 'static + Send + Clone) -> ActorRef<M> { //TODO single top-level actor?
        spawn_actor(&self.inner, behavior, None) //TODO synthetic root actor per ActorSystem
    }

    pub fn shutdown(&self) {
        let was_shutting_down = self.is_shutting_down.swap(true, Ordering::AcqRel);
        if !was_shutting_down {
            //TODO coordinated shutdown
            self.shut_down_completed.notify_waiters();
        }
    }

    pub async fn wait_for_shutdown(&self) {
        self.shut_down_completed.notified().await
    }
}

//TODO unit test: actor sending messages to itself until a 'stop doing this' message is sent to it from outside -> stack overflow?

#[cfg(test)]
mod test {
    use tracing::{info, Level};
    use tracing_subscriber::FmtSubscriber;
    use crate::messages::Signal;
    use crate::test_support::test_actor::TestKit;

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
    async fn test_asdf() {

        #[derive(Debug, Eq, PartialEq)]
        struct EchoMessage {
            msg: &'static str,
            sender: ActorRef<EchoMessage>,
        }
        fn echo_behavior(ctx: &mut ActorContext<EchoMessage>, m: EchoMessage) {
            m.sender.send(EchoMessage {
                msg: m.msg,
                sender: ctx.myself(),
            });
        }

        let actor_system = ActorSystem::new();
        let echo = actor_system.spawn(echo_behavior);

        let mut test_kit = TestKit::<EchoMessage>::new(&actor_system);

        echo.send(EchoMessage {
            msg: "hi",
            sender: test_kit.test_actor(),
        });

        let response = test_kit.expect_any_message().await;
        assert_eq!(response, EchoMessage { msg: "hi", sender: echo });
    }

    // #[tokio::test]
    async fn test_simple() {
        fn dumping_behavior(_ctx: &mut ActorContext<String>, s: String) {
            info!("{}", s);
        }

        fn dw_behavior(_ctx: &mut ActorContext<()>, _msg: ()) {}

        let actor_system = ActorSystem::new();
        let actor_ref = actor_system.spawn(dumping_behavior);

        let dw_ref = actor_system.spawn(dw_behavior);
        actor_ref.send_signal(Signal::RegisterDeathWatcher { subscriber: dw_ref.as_generic() });

        actor_ref.send("yo1".to_string());
        actor_ref.send("yo2".to_string());
        actor_ref.stop();
        actor_ref.send("yo3".to_string());

        tokio::time::sleep(Duration::from_secs(1)).await; //TODO
        actor_system.shutdown();

        // actor_system.wait_for_shutdown().await;
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
