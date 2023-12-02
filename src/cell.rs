use std::fmt::{Debug, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind};

use tracing::{debug, error, instrument, trace, warn};

use crate::messages::{Envelope, Signal};
use crate::behavior::ActorBehavior;
use crate::context::ActorContext;
use crate::mailbox::Mailbox;
use crate::refs::{GenericActorRef, SignalSender};
use crate::supervision::{CrashCause, CrashLifecycleStage, SupervisionStrategy, SupervisorDecision};

pub struct ActorCell<M: Send + 'static, S: SupervisionStrategy> {
    pub(crate) ctx: ActorContext<M>,
    pub(crate) mailbox: Mailbox<M>,
    pub(crate) is_terminating: bool,
    pub(crate) suspend_count: u32,

    pub(crate) behavior: Box<dyn ActorBehavior<M> + Send>,
    pub(crate) supervision_strategy: S,
}
impl <M: Send + Debug + 'static, S: SupervisionStrategy> ActorCell<M, S> {
    #[instrument]
    pub async fn message_loop(mut self) {
        trace!("starting message loop");
        while let Some(envelope) = self.mailbox.next(self.is_terminating || self.suspend_count > 0).await {
            trace!("received {:?}", envelope);
            match envelope {
                Envelope::Signal(Signal::ChildTerminated(child_ref)) => self.handle_child_terminated(&child_ref),
                Envelope::Signal(Signal::Restart) => self.handle_restart(),
                Envelope::Signal(Signal::Suspend) => self.handle_suspend(),
                Envelope::Signal(Signal::Terminate) => self.handle_terminate(),
                Envelope::Signal(Signal::SupervisionRequired(crashed_actor, cause, lifecycle_stage)) => self.handle_supervision_required(crashed_actor, cause, lifecycle_stage),
                Envelope::Signal(Signal::RegisterDeathWatcher { subscriber }) => {
                    if !self.ctx.death_watchers.insert(subscriber.clone()) {
                        debug!("registering death watcher {:?} that was registered already", subscriber);
                    }
                }
                Envelope::Signal(Signal::UnregisterDeathWatcher { subscriber }) => {
                    if !self.ctx.death_watchers.remove(&subscriber) {
                        debug!("unregistering death watcher {:?} that was not registered", subscriber);
                    }
                }
                _ => {
                    Self::do_with_supervision(&mut self.ctx, CrashLifecycleStage::Receive, |ctx| self.behavior.receive(ctx, envelope));
                }
            }

            if self.is_terminating && self.ctx.children.is_empty() {
                trace!("actually terminating actor");

                if let Some(parent) = self.ctx.parent {
                    parent.signal(Signal::ChildTerminated(self.ctx.myself.as_generic()));
                }

                if self.ctx.death_watchers.len() > 0 {
                    trace!("notifying {} death watchers", self.ctx.death_watchers.len());
                    for dw in &self.ctx.death_watchers {
                        dw.signal(Signal::Death(self.ctx.myself.id()));
                    }
                }
                break;
            }
        }
        trace!("exiting message loop");
    }

    fn do_with_supervision(self_ctx: &mut ActorContext<M>, lifecycle_stage: CrashLifecycleStage, f: impl FnOnce(&mut ActorContext<M>) -> anyhow::Result<()>) {
        let received = catch_unwind(AssertUnwindSafe(|| f(self_ctx)));
        let cause = match received {
            Err(e) => {
                // panicked -> actor crashed
                debug!("actor crashed by panicking in {:?}", lifecycle_stage);
                CrashCause::Panic(e)
            }
            Ok(Err(e)) => {
                // returned Err -> actor crashed
                debug!("actor crashed returning an error in {:?}: {}", lifecycle_stage, e);
                CrashCause::Err(e)
            }
            Ok(_) => {
                return;
            }
        };

        // this actor crashed - suspend it and ask supervisor
        self_ctx.myself.signal(Signal::Suspend);

        if let Some(supervisor) = &self_ctx.parent {
            supervisor.signal(Signal::SupervisionRequired(self_ctx.myself.as_generic(), cause, lifecycle_stage));
        }
        else {
            todo!()
        }
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
            //TODO replace behavior with original behavior
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
impl <M: Send + 'static, S: SupervisionStrategy> Debug for ActorCell<M, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ActorCell({})", self.ctx.myself.id())
    }
}
